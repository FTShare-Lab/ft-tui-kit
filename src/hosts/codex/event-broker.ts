import type {
  CanvasActionEvent,
  CanvasContextEvent,
  CanvasEventSink,
  CanvasSourceEvent,
  CanvasToolContext,
} from '../../canvas/manager/event-sink.ts';
import { CodexEventStore } from './event-store.ts';

const DEFAULT_MAX_EVENTS = 100;
const DEFAULT_MAX_BYTES = 256 * 1024;
const DEFAULT_TTL_MS = 30 * 60 * 1000;
const MAX_WAIT_MS = 55_000;

export interface CodexCanvasEvent {
  id: string;
  delivery: 'context' | 'action';
  sourceID: string;
  sourceEvent: CanvasSourceEvent;
  label?: string;
  text: string;
  data?: unknown;
  createdAt: number;
}

export interface CanvasWaitOptions {
  sourceID?: string;
  timeoutMs?: number;
  signal?: AbortSignal;
}

export interface CanvasWaitResult {
  reason: 'event' | 'timeout' | 'cancelled' | 'cleared';
  event?: CodexCanvasEvent;
}

interface EventWaiter {
  sourceID?: string;
  resolve: (result: CanvasWaitResult) => void;
  timeout: ReturnType<typeof setTimeout>;
  signal?: AbortSignal;
  onAbort?: () => void;
}

export interface CodexEventBrokerOptions {
  maxEvents?: number;
  maxBytes?: number;
  ttlMs?: number;
  store?: CodexEventStore;
}

export class CodexEventBroker implements CanvasEventSink {
  private readonly queues = new Map<string, CodexCanvasEvent[]>();
  private readonly waiters = new Map<string, EventWaiter[]>();
  private readonly latestContext = new Map<string, CanvasToolContext>();
  private readonly maxEvents: number;
  private readonly maxBytes: number;
  private readonly ttlMs: number;
  private readonly store: CodexEventStore;
  private sequence = 0;

  constructor(options: CodexEventBrokerOptions = {}) {
    this.maxEvents = options.maxEvents ?? DEFAULT_MAX_EVENTS;
    this.maxBytes = options.maxBytes ?? DEFAULT_MAX_BYTES;
    this.ttlMs = options.ttlMs ?? DEFAULT_TTL_MS;
    this.store = options.store ?? new CodexEventStore();
  }

  rememberToolContext(context: CanvasToolContext): void {
    this.latestContext.set(context.sessionID, context);
  }

  attachContext(sessionID: string, context: CanvasContextEvent): void {
    this.publish(sessionID, {
      id: this.nextID(),
      delivery: 'context',
      sourceID: context.sourceID,
      sourceEvent: context.sourceEvent,
      label: context.label,
      text: context.text,
      data: context.data,
      createdAt: context.createdAt,
    });
  }

  async enqueueAction(sessionID: string, action: CanvasActionEvent): Promise<void> {
    this.publish(sessionID, {
      id: this.nextID(),
      delivery: 'action',
      sourceID: action.sourceID,
      sourceEvent: action.sourceEvent,
      label: action.label,
      text: action.prompt,
      createdAt: action.createdAt,
    });
  }

  wait(sessionID: string, options: CanvasWaitOptions = {}): Promise<CanvasWaitResult> {
    this.prune(sessionID);
    const queued = this.takeQueued(sessionID, options.sourceID);
    if (queued) return Promise.resolve({ reason: 'event', event: queued });
    if (options.signal?.aborted) return Promise.resolve({ reason: 'cancelled' });

    const timeoutMs = Math.max(0, Math.min(options.timeoutMs ?? 30_000, MAX_WAIT_MS));
    if (timeoutMs === 0) return Promise.resolve({ reason: 'timeout' });

    return new Promise((resolve) => {
      const finish = (result: CanvasWaitResult): void => {
        this.removeWaiter(sessionID, waiter);
        resolve(result);
      };
      const waiter: EventWaiter = {
        sourceID: options.sourceID,
        resolve: finish,
        timeout: setTimeout(() => finish({ reason: 'timeout' }), timeoutMs),
        signal: options.signal,
      };
      if (options.signal) {
        waiter.onAbort = () => finish({ reason: 'cancelled' });
        options.signal.addEventListener('abort', waiter.onAbort, { once: true });
      }
      const items = this.waiters.get(sessionID) ?? [];
      items.push(waiter);
      this.waiters.set(sessionID, items);
    });
  }

  clear(sessionID: string): void {
    this.queues.delete(sessionID);
    this.latestContext.delete(sessionID);
    this.store.clear(sessionID);
    for (const waiter of [...(this.waiters.get(sessionID) ?? [])]) {
      waiter.resolve({ reason: 'cleared' });
    }
    this.waiters.delete(sessionID);
  }

  dispose(): void {
    const sessionIDs = new Set([
      ...this.queues.keys(),
      ...this.waiters.keys(),
      ...this.latestContext.keys(),
    ]);
    for (const sessionID of sessionIDs) {
      for (const waiter of [...(this.waiters.get(sessionID) ?? [])]) {
        waiter.resolve({ reason: 'cleared' });
      }
    }
    this.queues.clear();
    this.waiters.clear();
    this.latestContext.clear();
  }

  private publish(sessionID: string, event: CodexCanvasEvent): void {
    this.applyConsumedEvents(sessionID);
    const waiters = this.waiters.get(sessionID) ?? [];
    const waiter = waiters.find((item) => !item.sourceID || item.sourceID === event.sourceID);
    if (waiter) {
      waiter.resolve({ reason: 'event', event });
      return;
    }

    const queue = this.queues.get(sessionID) ?? [];
    queue.push(event);
    this.queues.set(sessionID, queue);
    this.store.record(sessionID, event);
    this.prune(sessionID);
  }

  private takeQueued(sessionID: string, sourceID?: string): CodexCanvasEvent | undefined {
    const queue = this.queues.get(sessionID);
    if (!queue?.length) return undefined;

    const index = sourceID ? queue.findIndex((event) => event.sourceID === sourceID) : 0;
    if (index < 0) return undefined;

    const [event] = queue.splice(index, 1);
    this.store.acknowledge(sessionID, event.id);
    if (!queue.length) this.queues.delete(sessionID);
    return event;
  }

  private prune(sessionID: string): void {
    this.applyConsumedEvents(sessionID);
    const queue = this.queues.get(sessionID);
    if (!queue?.length) return;

    const expiresBefore = Date.now() - this.ttlMs;
    while (queue[0] && queue[0].createdAt < expiresBefore) queue.shift();
    while (queue.length > this.maxEvents) queue.shift();
    while (queue.length > 1 && this.queueBytes(queue) > this.maxBytes) queue.shift();

    if (!queue.length) this.queues.delete(sessionID);
  }

  private applyConsumedEvents(sessionID: string): void {
    const consumed = this.store.takeConsumedIDs(sessionID);
    if (!consumed.size) return;
    const queue = this.queues.get(sessionID);
    if (!queue) return;
    const remaining = queue.filter((event) => !consumed.has(event.id));
    if (remaining.length) this.queues.set(sessionID, remaining);
    else this.queues.delete(sessionID);
  }

  private removeWaiter(sessionID: string, waiter: EventWaiter): void {
    clearTimeout(waiter.timeout);
    if (waiter.signal && waiter.onAbort) {
      waiter.signal.removeEventListener('abort', waiter.onAbort);
    }

    const items = this.waiters.get(sessionID);
    if (!items) return;
    const index = items.indexOf(waiter);
    if (index >= 0) items.splice(index, 1);
    if (!items.length) this.waiters.delete(sessionID);
  }

  private queueBytes(queue: CodexCanvasEvent[]): number {
    return queue.reduce((total, event) => total + Buffer.byteLength(JSON.stringify(event)), 0);
  }

  private nextID(): string {
    this.sequence = (this.sequence + 1) % Number.MAX_SAFE_INTEGER;
    return `canvas_evt_${Date.now().toString(36)}_${this.sequence.toString(36)}`;
  }
}
