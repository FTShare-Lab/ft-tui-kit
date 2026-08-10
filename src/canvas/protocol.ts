export const CANVAS_PROTOCOL_VERSION = 2 as const;
export const MAX_FRAME_BYTES = 256 * 1024;

export type RuntimeChannel = 'control' | 'event';
export type PromptDelivery = 'context' | 'steer' | 'queue';
export type ActionDelivery = Exclude<PromptDelivery, 'context'>;

export interface RendererCapabilities {
  state?: boolean;
  selection?: boolean;
  content?: boolean;
  context?: boolean;
  action?: boolean;
  artifacts?: boolean;
  command?: boolean;
}

export interface RendererLaunchConfig {
  version: typeof CANVAS_PROTOCOL_VERSION;
  widgetId: string;
  kind: string;
  scenario: string;
  title?: string;
  token: string;
  runtimeDir: string;
  controlSocketPath: string;
  eventSocketPath: string;
  configPath: string;
  manifest: {
    name: string;
    description?: string;
    defaultScenario: string;
    capabilities: RendererCapabilities;
  };
}

export interface RuntimeFrame<
  TChannel extends RuntimeChannel = RuntimeChannel,
  TType extends string = string,
  TPayload = unknown,
> {
  version: typeof CANVAS_PROTOCOL_VERSION;
  id: string;
  widgetId: string;
  channel: TChannel;
  type: TType;
  timestamp: number;
  requestId?: string;
  payload: TPayload;
}

export type HostControlFrame =
  | RuntimeFrame<'control', 'init', { launch: RendererLaunchConfig; config: unknown }>
  | RuntimeFrame<'control', 'update', { config: unknown }>
  | RuntimeFrame<'control', 'focus', { active: boolean; focused?: boolean }>
  | RuntimeFrame<'control', 'registry', { activeId?: string; widgets: CanvasRegistryEntry[] }>
  | RuntimeFrame<'control', 'request.state', { key?: string }>
  | RuntimeFrame<'control', 'request.selection', Record<string, never>>
  | RuntimeFrame<'control', 'request.content', Record<string, never>>
  | RuntimeFrame<'control', 'close', { reason?: string }>
  | RuntimeFrame<'control', 'ping', Record<string, never>>;

export type HostEventFrame =
  | RuntimeFrame<'event', 'event.ack', { eventId: string }>
  | RuntimeFrame<'event', 'event.nack', { eventId: string; error: string }>
  | RuntimeFrame<'event', 'backpressure', { reason: string }>
  | RuntimeFrame<'event', 'prompt.delivered', { eventId: string }>
  | RuntimeFrame<'event', 'context.consumed', { eventIds: string[] }>;

export type HostFrame = HostControlFrame | HostEventFrame;

export type RendererControlFrame =
  | RuntimeFrame<
      'control',
      'hello',
      { token: string; kind: string; scenario: string; pid?: number }
    >
  | RuntimeFrame<'control', 'ready', { title?: string; capabilities?: RendererCapabilities }>
  | RuntimeFrame<'control', 'capabilities', RendererCapabilities>
  | RuntimeFrame<
      'control',
      'rpc.response',
      { ok: true; data?: unknown } | { ok: false; error: string }
    >
  | RuntimeFrame<'control', 'error', { message: string; fatal?: boolean }>
  | RuntimeFrame<'control', 'pong', Record<string, never>>;

export type RendererEventFrame =
  | RuntimeFrame<'event', 'hello', { token: string; kind: string; scenario: string; pid?: number }>
  | RuntimeFrame<'event', 'state', { key?: string; label?: string; data: unknown }>
  | RuntimeFrame<
      'event',
      'selection',
      { label?: string; text?: string; prompt?: string; data?: unknown; delivery?: PromptDelivery }
    >
  | RuntimeFrame<
      'event',
      'context',
      { label?: string; text: string; data?: unknown; delivery?: PromptDelivery }
    >
  | RuntimeFrame<
      'event',
      'action',
      { label?: string; prompt?: string; text?: string; data?: unknown; delivery?: ActionDelivery }
    >
  | RuntimeFrame<
      'event',
      'artifact',
      {
        label?: string;
        uri?: string;
        path?: string;
        mediaType?: string;
        text?: string;
        data?: unknown;
        delivery?: PromptDelivery;
      }
    >
  | RuntimeFrame<'event', 'command', { name: string; data?: unknown }>
  | RuntimeFrame<'event', 'control', { command: 'switch' | 'next' | 'close'; targetId?: string }>
  | RuntimeFrame<'event', 'cancelled', { reason?: string }>
  | RuntimeFrame<'event', 'error', { message: string; fatal?: boolean }>
  | RuntimeFrame<
      'event',
      'log',
      { level?: 'debug' | 'info' | 'warn' | 'error'; message: string; data?: unknown }
    >;

export type RendererFrame = RendererControlFrame | RendererEventFrame;

export interface CanvasRegistryEntry {
  id: string;
  kind: string;
  scenario: string;
  title?: string;
  status: 'starting' | 'ready' | 'closed' | 'error';
  active: boolean;
  visible?: boolean;
  focused?: boolean;
  capabilities?: RendererCapabilities;
}

export interface CanvasSelection {
  selectedText?: string;
  startOffset?: number;
  endOffset?: number;
  [key: string]: unknown;
}

export interface CanvasContent {
  content?: string;
  cursorPosition?: number;
  [key: string]: unknown;
}

export function createId(prefix: string): string {
  return `${prefix}_${crypto.randomUUID().replaceAll('-', '')}`;
}

export function createFrame<TChannel extends RuntimeChannel, TType extends string, TPayload>(
  channel: TChannel,
  widgetId: string,
  type: TType,
  payload: TPayload,
  requestId?: string
): RuntimeFrame<TChannel, TType, TPayload> {
  return {
    version: CANVAS_PROTOCOL_VERSION,
    id: createId(channel === 'control' ? 'ctl' : 'evt'),
    widgetId,
    channel,
    type,
    timestamp: Date.now(),
    requestId,
    payload,
  };
}

export function encodeFrame(frame: RuntimeFrame): string {
  return `${JSON.stringify(frame)}\n`;
}

export function parseFrame(line: string, expectedChannel?: RuntimeChannel): RuntimeFrame {
  const parsed = JSON.parse(line) as unknown;
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Frame is not an object');
  }

  const frame = parsed as Partial<RuntimeFrame>;
  if (frame.version !== CANVAS_PROTOCOL_VERSION) {
    throw new Error(`Unsupported protocol version: ${String(frame.version)}`);
  }
  if (typeof frame.id !== 'string' || !frame.id) {
    throw new Error('Frame id is required');
  }
  if (typeof frame.widgetId !== 'string' || !frame.widgetId) {
    throw new Error('Frame widgetId is required');
  }
  if (frame.channel !== 'control' && frame.channel !== 'event') {
    throw new Error('Frame channel must be control or event');
  }
  if (expectedChannel && frame.channel !== expectedChannel) {
    throw new Error(`Frame channel mismatch: expected ${expectedChannel}, got ${frame.channel}`);
  }
  if (typeof frame.type !== 'string' || !frame.type) {
    throw new Error('Frame type is required');
  }
  if (typeof frame.timestamp !== 'number') {
    throw new Error('Frame timestamp is required');
  }

  return frame as RuntimeFrame;
}
