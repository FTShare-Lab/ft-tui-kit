import {
  MAX_FRAME_BYTES,
  createFrame,
  encodeFrame,
  parseFrame,
  type HostControlFrame,
  type HostEventFrame,
  type RendererControlFrame,
  type RendererEventFrame,
  type RendererLaunchConfig,
  type RuntimeChannel,
  type RuntimeFrame,
} from '../../../src/canvas/protocol.ts';

type SocketLike = {
  write(data: string): unknown;
  end(): void;
};

export interface RendererClientOptions {
  launch: RendererLaunchConfig;
  onControlFrame: (frame: HostControlFrame) => void;
  onEventFrame?: (frame: HostEventFrame) => void;
  onDisconnect?: (channel: RuntimeChannel) => void;
  onError?: (error: Error) => void;
}

export interface RendererClient {
  sendControl(frame: RendererControlFrame): void;
  sendEvent(frame: RendererEventFrame): void;
  close(): void;
  isControlConnected(): boolean;
  isEventConnected(): boolean;
}

export async function connectRendererClient(
  options: RendererClientOptions
): Promise<RendererClient> {
  let controlConnected = false;
  let eventConnected = false;
  let stopped = false;

  const control = await connectChannel({
    channel: 'control',
    socketPath: options.launch.controlSocketPath,
    launch: options.launch,
    onFrame: (frame) => options.onControlFrame(frame as HostControlFrame),
    onConnected: () => {
      controlConnected = true;
    },
    onDisconnect: () => {
      controlConnected = false;
      if (!stopped) options.onDisconnect?.('control');
    },
    onError: options.onError,
  });

  const events = await connectChannel({
    channel: 'event',
    socketPath: options.launch.eventSocketPath,
    launch: options.launch,
    onFrame: (frame) => options.onEventFrame?.(frame as HostEventFrame),
    onConnected: () => {
      eventConnected = true;
    },
    onDisconnect: () => {
      eventConnected = false;
      if (!stopped) options.onDisconnect?.('event');
    },
    onError: options.onError,
  });

  return {
    sendControl(frame) {
      if (controlConnected) {
        control.write(encodeFrame(frame));
      }
    },
    sendEvent(frame) {
      if (eventConnected) {
        events.write(encodeFrame(frame));
      }
    },
    close() {
      stopped = true;
      controlConnected = false;
      eventConnected = false;
      control.end();
      events.end();
    },
    isControlConnected() {
      return controlConnected;
    },
    isEventConnected() {
      return eventConnected;
    },
  };
}

interface ConnectChannelOptions {
  channel: RuntimeChannel;
  socketPath: string;
  launch: RendererLaunchConfig;
  onFrame: (frame: RuntimeFrame) => void;
  onConnected: () => void;
  onDisconnect: () => void;
  onError?: (error: Error) => void;
}

async function connectChannel(options: ConnectChannelOptions): Promise<SocketLike> {
  let lastError: Error | undefined;
  for (let attempt = 0; attempt < 20; attempt++) {
    try {
      return await connectOnce(options);
    } catch (error) {
      lastError = error instanceof Error ? error : new Error(String(error));
      await sleep(Math.min(100 * 2 ** attempt, 1000));
    }
  }

  throw lastError ?? new Error(`Unable to connect ${options.channel} socket`);
}

function connectOnce(options: ConnectChannelOptions): Promise<SocketLike> {
  return new Promise((resolve, reject) => {
    let buffer = '';
    let opened = false;

    Bun.connect({
      unix: options.socketPath,
      socket: {
        open(socket) {
          opened = true;
          options.onConnected();
          socket.write(
            encodeFrame(
              createFrame(options.channel, options.launch.widgetId, 'hello', {
                token: options.launch.token,
                kind: options.launch.kind,
                scenario: options.launch.scenario,
                pid: process.pid,
              })
            )
          );
          resolve(socket);
        },
        data(_socket, data) {
          buffer += data.toString();
          if (Buffer.byteLength(buffer) > MAX_FRAME_BYTES) {
            reject(new Error(`Host ${options.channel} frame exceeded ${MAX_FRAME_BYTES} bytes`));
            return;
          }

          const lines = buffer.split('\n');
          buffer = lines.pop() ?? '';

          for (const line of lines) {
            if (!line.trim()) continue;
            try {
              options.onFrame(parseFrame(line, options.channel));
            } catch (error) {
              options.onError?.(error instanceof Error ? error : new Error(String(error)));
            }
          }
        },
        close() {
          options.onDisconnect();
        },
        error(_socket, error) {
          if (!opened) {
            reject(error);
            return;
          }
          options.onError?.(error);
        },
      },
    }).catch((error: unknown) => {
      reject(error instanceof Error ? error : new Error(String(error)));
    });
  });
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
