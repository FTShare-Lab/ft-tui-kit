// Renderer-side IPC hook for the v2 two-socket host protocol.

import { useCallback, useEffect, useRef, useState } from 'react';
import { useApp } from 'ink';
import { connectRendererClient, type RendererClient } from './renderer-client.ts';
import {
  createFrame,
  type ActionDelivery,
  type CanvasContent,
  type CanvasRegistryEntry,
  type CanvasSelection,
  type HostControlFrame,
  type PromptDelivery,
  type RendererControlFrame,
  type RendererEventFrame,
  type RendererLaunchConfig,
} from '../../../src/canvas/protocol.ts';

export interface UseIPCOptions {
  launch?: RendererLaunchConfig;
  scenario: string;
  title?: string;
  onClose?: () => void;
  onUpdate?: (config: unknown) => void;
  onGetSelection?: () => CanvasSelection | null;
  onGetContent?: () => CanvasContent | undefined;
  onGetState?: (key?: string) => unknown;
  onRegistry?: (widgets: CanvasRegistryEntry[], activeID?: string) => void;
  onFocus?: (active: boolean, focused?: boolean) => void;
  onError?: (error: Error) => void;
}

export interface IPCHandle {
  isConnected: boolean;
  sendReady: () => void;
  sendSelected: (
    data: unknown,
    options?: { text?: string; prompt?: string; label?: string; delivery?: PromptDelivery }
  ) => void;
  sendCancelled: (reason?: string) => void;
  sendError: (message: string) => void;
  sendContext: (
    text: string,
    options?: { label?: string; data?: unknown; delivery?: PromptDelivery }
  ) => void;
  sendAction: (
    prompt: string,
    options?: { label?: string; data?: unknown; delivery?: ActionDelivery }
  ) => void;
  sendCommand: (name: string, data?: unknown) => void;
  sendState: (key: string, value: unknown) => void;
  sendControl: (command: 'switch' | 'next' | 'close', targetID?: string) => void;
}

export function useIPC(options: UseIPCOptions): IPCHandle {
  const { launch, title } = options;
  const { exit } = useApp();
  const [isConnected, setIsConnected] = useState(false);
  const clientRef = useRef<RendererClient | null>(null);
  const optionsRef = useRef(options);

  useEffect(() => {
    optionsRef.current = options;
  }, [options]);

  useEffect(() => {
    if (!launch) return;

    let mounted = true;
    let localClient: RendererClient | undefined;

    const connect = async (): Promise<void> => {
      try {
        const client = await connectRendererClient({
          launch,
          onControlFrame: (frame) => {
            handleControlFrame(frame, clientRef.current, optionsRef.current, exit);
          },
          onEventFrame: () => {},
          onDisconnect: () => {
            if (mounted) setIsConnected(false);
          },
          onError: (error) => {
            optionsRef.current.onError?.(error);
          },
        });

        localClient = client;
        if (!mounted) {
          client.close();
          return;
        }

        clientRef.current = client;
        setIsConnected(client.isControlConnected() && client.isEventConnected());
        sendControlFrame(launch, client, 'ready', {
          title,
          capabilities: launch.manifest.capabilities,
        });
      } catch (error) {
        optionsRef.current.onError?.(error instanceof Error ? error : new Error(String(error)));
      }
    };

    void connect();

    return () => {
      mounted = false;
      localClient?.close();
      clientRef.current = null;
    };
  }, [launch, title, exit]);

  const sendReady = useCallback(() => {
    if (!launch || !clientRef.current) return;
    sendControlFrame(launch, clientRef.current, 'ready', {
      title,
      capabilities: launch.manifest.capabilities,
    });
  }, [launch, title]);

  const sendSelected = useCallback(
    (
      data: unknown,
      sendOptions?: { text?: string; prompt?: string; label?: string; delivery?: PromptDelivery }
    ) => {
      if (!launch || !clientRef.current) return;
      sendEventFrame(launch, clientRef.current, 'selection', {
        data,
        text: sendOptions?.text,
        prompt: sendOptions?.prompt,
        label: sendOptions?.label,
        delivery: sendOptions?.delivery,
      });
    },
    [launch]
  );

  const sendContext = useCallback(
    (text: string, sendOptions?: { label?: string; data?: unknown; delivery?: PromptDelivery }) => {
      if (!launch || !clientRef.current) return;
      sendEventFrame(launch, clientRef.current, 'context', {
        text,
        label: sendOptions?.label,
        data: sendOptions?.data,
        delivery: sendOptions?.delivery,
      });
    },
    [launch]
  );

  const sendAction = useCallback(
    (
      prompt: string,
      sendOptions?: { label?: string; data?: unknown; delivery?: ActionDelivery }
    ) => {
      if (!launch || !clientRef.current) return;
      sendEventFrame(launch, clientRef.current, 'action', {
        prompt,
        label: sendOptions?.label,
        data: sendOptions?.data,
        delivery: sendOptions?.delivery,
      });
    },
    [launch]
  );

  const sendState = useCallback(
    (key: string, value: unknown) => {
      if (!launch || !clientRef.current) return;
      sendEventFrame(launch, clientRef.current, 'state', { key, data: value });
    },
    [launch]
  );

  const sendCommand = useCallback(
    (name: string, data?: unknown) => {
      if (!launch || !clientRef.current) return;
      sendEventFrame(launch, clientRef.current, 'command', { name, data });
    },
    [launch]
  );

  const sendCancelled = useCallback(
    (reason?: string) => {
      if (!launch || !clientRef.current) return;
      sendEventFrame(launch, clientRef.current, 'cancelled', { reason });
    },
    [launch]
  );

  const sendError = useCallback(
    (message: string) => {
      if (!launch || !clientRef.current) return;
      sendEventFrame(launch, clientRef.current, 'error', { message });
    },
    [launch]
  );

  const sendControl = useCallback(
    (command: 'switch' | 'next' | 'close', targetID?: string) => {
      if (!launch || !clientRef.current) return;
      sendEventFrame(launch, clientRef.current, 'control', { command, targetId: targetID });
    },
    [launch]
  );

  return {
    isConnected,
    sendReady,
    sendSelected,
    sendCancelled,
    sendError,
    sendContext,
    sendAction,
    sendCommand,
    sendState,
    sendControl,
  };
}

function handleControlFrame(
  frame: HostControlFrame,
  client: RendererClient | null,
  options: UseIPCOptions,
  exit: () => void
): void {
  switch (frame.type) {
    case 'init':
      options.onUpdate?.(frame.payload.config);
      return;
    case 'update':
      applyConfigUpdate(frame, client, options);
      return;
    case 'focus':
      options.onFocus?.(frame.payload.active, frame.payload.focused);
      return;
    case 'registry':
      options.onRegistry?.(frame.payload.widgets, frame.payload.activeId);
      return;
    case 'request.state':
      sendRpcResponse(frame, client, options.onGetState?.(frame.payload.key));
      return;
    case 'request.selection':
      sendRpcResponse(frame, client, options.onGetSelection?.() ?? null);
      return;
    case 'request.content':
      sendRpcResponse(frame, client, options.onGetContent?.());
      return;
    case 'close':
      options.onClose?.();
      exit();
      return;
    case 'ping':
      client?.sendControl(
        createFrame('control', frame.widgetId, 'pong', {}) as RendererControlFrame
      );
      return;
  }
}

function applyConfigUpdate(
  frame: Extract<HostControlFrame, { type: 'update' }>,
  client: RendererClient | null,
  options: UseIPCOptions
): void {
  try {
    options.onUpdate?.(frame.payload.config);
    client?.sendControl(
      createFrame(
        'control',
        frame.widgetId,
        'ready',
        {
          title: options.title,
          capabilities: options.launch?.manifest.capabilities,
        },
        frame.requestId
      ) as RendererControlFrame
    );
  } catch (error) {
    const resolved = error instanceof Error ? error : new Error(String(error));
    client?.sendControl(
      createFrame(
        'control',
        frame.widgetId,
        'error',
        { message: resolved.message, fatal: false },
        frame.requestId
      ) as RendererControlFrame
    );
    options.onError?.(resolved);
  }
}

function sendRpcResponse(
  frame: HostControlFrame,
  client: RendererClient | null,
  data: unknown
): void {
  if (!frame.requestId) return;
  client?.sendControl(
    createFrame(
      'control',
      frame.widgetId,
      'rpc.response',
      { ok: true, data },
      frame.requestId
    ) as RendererControlFrame
  );
}

function sendControlFrame(
  launch: RendererLaunchConfig,
  client: RendererClient,
  type: RendererControlFrame['type'],
  payload: RendererControlFrame['payload']
): void {
  client.sendControl(
    createFrame('control', launch.widgetId, type, payload) as RendererControlFrame
  );
}

function sendEventFrame(
  launch: RendererLaunchConfig,
  client: RendererClient,
  type: RendererEventFrame['type'],
  payload: RendererEventFrame['payload']
): void {
  client.sendEvent(createFrame('event', launch.widgetId, type, payload) as RendererEventFrame);
}
