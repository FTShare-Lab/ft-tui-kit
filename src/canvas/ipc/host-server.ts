import { chmodSync, existsSync, unlinkSync } from 'fs';
import {
  MAX_FRAME_BYTES,
  createFrame,
  createId,
  encodeFrame,
  parseFrame,
  type HostControlFrame,
  type HostEventFrame,
  type RendererControlFrame,
  type RendererEventFrame,
  type RendererFrame,
  type RendererLaunchConfig,
  type RuntimeChannel,
  type RuntimeFrame,
} from '../protocol.ts';

type SocketLike = {
  write(data: string): unknown;
  end(): void;
};

type BunServerLike = {
  stop(): void;
};

interface PendingRequest {
  resolve(data: unknown): void;
  reject(error: Error): void;
  timeout: ReturnType<typeof setTimeout>;
}

export interface RendererHostServerOptions {
  widgetId: string;
  token: string;
  controlSocketPath: string;
  eventSocketPath: string;
  launch: RendererLaunchConfig;
  config: unknown;
  onControlMessage: (frame: RendererControlFrame) => void | Promise<void>;
  onEventMessage: (frame: RendererEventFrame) => void | Promise<void>;
  onDisconnect?: (channel: RuntimeChannel) => void | Promise<void>;
  onError?: (error: Error) => void;
}

export interface RendererHostServer {
  sendControl(frame: HostControlFrame): void;
  sendEvent(frame: HostEventFrame): void;
  request(
    method: 'state' | 'selection' | 'content',
    payload?: Record<string, unknown>,
    timeoutMs?: number
  ): Promise<unknown>;
  isControlConnected(): boolean;
  isEventConnected(): boolean;
  close(reason?: string): void;
}

export async function createRendererHostServer(
  options: RendererHostServerOptions
): Promise<RendererHostServer> {
  const pending = new Map<string, PendingRequest>();
  let controlPeer: SocketLike | undefined;
  let eventPeer: SocketLike | undefined;

  const sendControl = (frame: HostControlFrame): void => {
    controlPeer?.write(encodeFrame(frame));
  };

  const sendEvent = (frame: HostEventFrame): void => {
    eventPeer?.write(encodeFrame(frame));
  };

  const controlServer = createChannelServer({
    channel: 'control',
    socketPath: options.controlSocketPath,
    widgetId: options.widgetId,
    token: options.token,
    onAuthenticated(socket) {
      controlPeer = socket;
      sendControl(
        createFrame('control', options.widgetId, 'init', {
          launch: options.launch,
          config: options.config,
        }) as HostControlFrame
      );
    },
    onFrame(frame) {
      const controlFrame = frame as RendererControlFrame;
      if (controlFrame.type === 'rpc.response' && controlFrame.requestId) {
        const request = pending.get(controlFrame.requestId);
        if (request) {
          pending.delete(controlFrame.requestId);
          clearTimeout(request.timeout);
          if (controlFrame.payload.ok) {
            request.resolve(controlFrame.payload.data);
          } else {
            request.reject(new Error(controlFrame.payload.error));
          }
        }
        return;
      }

      return options.onControlMessage(controlFrame);
    },
    onDisconnect() {
      controlPeer = undefined;
      return options.onDisconnect?.('control');
    },
    onError: options.onError,
  });

  const eventServer = createChannelServer({
    channel: 'event',
    socketPath: options.eventSocketPath,
    widgetId: options.widgetId,
    token: options.token,
    onAuthenticated(socket) {
      eventPeer = socket;
    },
    async onFrame(frame) {
      const eventFrame = frame as RendererEventFrame;
      try {
        await options.onEventMessage(eventFrame);
        sendEvent(
          createFrame('event', options.widgetId, 'event.ack', {
            eventId: eventFrame.id,
          }) as HostEventFrame
        );
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        sendEvent(
          createFrame('event', options.widgetId, 'event.nack', {
            eventId: eventFrame.id,
            error: message,
          }) as HostEventFrame
        );
        throw error;
      }
    },
    onDisconnect() {
      eventPeer = undefined;
      return options.onDisconnect?.('event');
    },
    onError: options.onError,
  });

  return {
    sendControl,
    sendEvent,
    request(method, payload = {}, timeoutMs = 2000) {
      if (!controlPeer) {
        return Promise.reject(new Error('Renderer control channel is not connected'));
      }

      const requestId = createId('req');
      const frameType =
        method === 'state'
          ? 'request.state'
          : method === 'selection'
            ? 'request.selection'
            : 'request.content';

      return new Promise((resolve, reject) => {
        const timeout = setTimeout(() => {
          pending.delete(requestId);
          reject(new Error(`Timed out waiting for renderer ${method} response`));
        }, timeoutMs);

        pending.set(requestId, { resolve, reject, timeout });
        sendControl(
          createFrame(
            'control',
            options.widgetId,
            frameType,
            payload,
            requestId
          ) as HostControlFrame
        );
      });
    },
    isControlConnected() {
      return controlPeer !== undefined;
    },
    isEventConnected() {
      return eventPeer !== undefined;
    },
    close(reason) {
      if (controlPeer) {
        sendControl(
          createFrame('control', options.widgetId, 'close', { reason }) as HostControlFrame
        );
      }

      for (const request of pending.values()) {
        clearTimeout(request.timeout);
        request.reject(new Error('Renderer host server closed'));
      }
      pending.clear();

      controlPeer?.end();
      eventPeer?.end();
      controlPeer = undefined;
      eventPeer = undefined;
      controlServer.stop();
      eventServer.stop();
      unlinkIfExists(options.controlSocketPath);
      unlinkIfExists(options.eventSocketPath);
    },
  };
}

interface ChannelServerOptions {
  channel: RuntimeChannel;
  socketPath: string;
  widgetId: string;
  token: string;
  onAuthenticated(socket: SocketLike): void;
  onFrame(frame: RendererFrame): void | Promise<void>;
  onDisconnect?: () => void | Promise<void>;
  onError?: (error: Error) => void;
}

function createChannelServer(options: ChannelServerOptions): BunServerLike {
  unlinkIfExists(options.socketPath);

  const clients = new Set<SocketLike>();
  const buffers = new WeakMap<object, string>();
  const authenticated = new WeakSet<object>();

  const server = Bun.listen({
    unix: options.socketPath,
    socket: {
      open(socket) {
        clients.add(socket);
        buffers.set(socket, '');
      },
      data(socket, data) {
        const next = `${buffers.get(socket) ?? ''}${data.toString()}`;
        if (Buffer.byteLength(next) > MAX_FRAME_BYTES) {
          socket.end();
          options.onError?.(new Error(`IPC frame exceeded ${MAX_FRAME_BYTES} bytes`));
          return;
        }

        const lines = next.split('\n');
        buffers.set(socket, lines.pop() ?? '');

        for (const line of lines) {
          if (!line.trim()) continue;

          try {
            const frame = parseFrame(line, options.channel);
            if (frame.widgetId !== options.widgetId) {
              throw new Error('Frame widgetId does not match socket owner');
            }

            if (!authenticated.has(socket)) {
              authenticate(socket, frame, options);
              continue;
            }

            void Promise.resolve(options.onFrame(frame as RendererFrame)).catch(
              (error: unknown) => {
                options.onError?.(error instanceof Error ? error : new Error(String(error)));
              }
            );
          } catch (error) {
            options.onError?.(error instanceof Error ? error : new Error(String(error)));
            socket.end();
          }
        }
      },
      close(socket) {
        clients.delete(socket);
        buffers.delete(socket);
        void Promise.resolve(options.onDisconnect?.()).catch((error: unknown) => {
          options.onError?.(error instanceof Error ? error : new Error(String(error)));
        });
      },
      error(_socket, error) {
        options.onError?.(error);
      },
    },
  });

  try {
    chmodSync(options.socketPath, 0o600);
  } catch {
    // chmod is best-effort for portability.
  }

  function stop(): void {
    for (const client of clients) {
      client.end();
    }
    clients.clear();
    server.stop();
  }

  function authenticate(
    socket: SocketLike,
    frame: RuntimeFrame,
    input: ChannelServerOptions
  ): void {
    if (frame.type !== 'hello') {
      throw new Error(`First ${input.channel} frame must be hello`);
    }

    const payload = frame.payload as { token?: unknown };
    if (payload.token !== input.token) {
      throw new Error(`${input.channel} channel authentication failed`);
    }

    authenticated.add(socket);
    input.onAuthenticated(socket);
  }

  return { stop };
}

function unlinkIfExists(file: string): void {
  if (existsSync(file)) {
    unlinkSync(file);
  }
}
