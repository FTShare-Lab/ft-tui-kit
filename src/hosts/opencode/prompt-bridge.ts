import type { PluginInput } from '@opencode-ai/plugin';
import type {
  CanvasActionEvent,
  CanvasContextEvent,
  CanvasEventSink,
  CanvasToolContext,
} from '../../canvas/manager/event-sink.ts';

interface ModelRef {
  providerID: string;
  modelID: string;
}

interface SessionPromptContext {
  agent?: string;
  model?: ModelRef;
  variant?: string;
}

interface ChatMessageInput {
  sessionID: string;
  agent?: string;
  model?: ModelRef;
  variant?: string;
  messageID?: string;
}

interface ChatMessageOutput {
  message: {
    id?: string;
    agent?: string;
    model?: ModelRef & { variant?: string };
  };
  parts: Array<Record<string, unknown>>;
}

interface OpenCodeEvent {
  type: string;
  properties?: Record<string, unknown>;
}

function makePartID(): string {
  return `prt_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`;
}

function compactJson(value: unknown): string | undefined {
  if (value === undefined) return undefined;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function isFailureResponse(value: unknown): boolean {
  if (!value || typeof value !== 'object') return false;
  return 'error' in value && Boolean((value as { error?: unknown }).error);
}

export class PromptBridge implements CanvasEventSink {
  private readonly contextBySession = new Map<string, CanvasContextEvent[]>();
  private readonly actionsBySession = new Map<string, CanvasActionEvent[]>();
  private readonly promptContextBySession = new Map<string, SessionPromptContext>();
  private readonly busySessions = new Set<string>();

  constructor(private readonly input: PluginInput) {}

  rememberToolContext(context: CanvasToolContext): void {
    this.promptContextBySession.set(context.sessionID, {
      agent: context.agent,
    });
    this.busySessions.add(context.sessionID);
  }

  attachContext(sessionID: string, context: CanvasContextEvent): void {
    const items = this.contextBySession.get(sessionID) ?? [];
    items.push(context);
    this.contextBySession.set(sessionID, items);
  }

  async enqueueAction(sessionID: string, action: CanvasActionEvent): Promise<void> {
    const items = this.actionsBySession.get(sessionID) ?? [];
    items.push(action);
    this.actionsBySession.set(sessionID, items);
    await this.flush(sessionID);
  }

  async onChatMessage(input: ChatMessageInput, output: ChatMessageOutput): Promise<void> {
    this.promptContextBySession.set(input.sessionID, {
      agent: input.agent ?? output.message.agent,
      model: input.model ?? output.message.model,
      variant: input.variant ?? output.message.model?.variant,
    });

    const contexts = this.contextBySession.get(input.sessionID);
    if (!contexts?.length) return;

    this.contextBySession.delete(input.sessionID);
    const text = this.formatContext(contexts);

    output.parts.push({
      id: makePartID(),
      messageID: output.message.id ?? input.messageID,
      sessionID: input.sessionID,
      type: 'text',
      text,
      synthetic: true,
    });
  }

  async onOpenCodeEvent(event: OpenCodeEvent): Promise<void> {
    if (event.type === 'session.deleted') {
      const sessionID = this.readSessionID(event);
      if (sessionID) this.clear(sessionID);
      return;
    }

    const sessionID = this.readSessionID(event);
    if (!sessionID) return;

    if (event.type === 'session.status') {
      const status = event.properties?.status;
      const statusType =
        status && typeof status === 'object' && 'type' in status
          ? (status as { type?: unknown }).type
          : undefined;

      if (statusType === 'idle') {
        this.busySessions.delete(sessionID);
        await this.flush(sessionID);
        return;
      }

      this.busySessions.add(sessionID);
      return;
    }

    if (event.type === 'session.idle') {
      this.busySessions.delete(sessionID);
      await this.flush(sessionID);
    }
  }

  clear(sessionID: string): void {
    this.contextBySession.delete(sessionID);
    this.actionsBySession.delete(sessionID);
    this.promptContextBySession.delete(sessionID);
    this.busySessions.delete(sessionID);
  }

  private async flush(sessionID: string): Promise<void> {
    if (this.busySessions.has(sessionID)) return;

    const queue = this.actionsBySession.get(sessionID);
    if (!queue?.length) {
      this.actionsBySession.delete(sessionID);
      return;
    }

    const next = queue.shift()!;
    if (!queue.length) {
      this.actionsBySession.delete(sessionID);
    }

    this.busySessions.add(sessionID);

    try {
      await this.promptAsync(sessionID, next.prompt);
    } catch (error) {
      this.busySessions.delete(sessionID);
      const retryQueue = this.actionsBySession.get(sessionID) ?? [];
      retryQueue.unshift(next);
      this.actionsBySession.set(sessionID, retryQueue);
      throw error;
    }
  }

  private async promptAsync(sessionID: string, prompt: string): Promise<void> {
    const context = this.promptContextBySession.get(sessionID);
    const client = this.input.client as unknown as {
      session?: {
        promptAsync?: (input: unknown) => Promise<unknown>;
      };
    };
    const sessionClient = client.session;
    const promptAsync = sessionClient?.promptAsync?.bind(sessionClient);
    if (!promptAsync) {
      throw new Error('OpenCode client does not expose session.promptAsync');
    }

    const flatPayload = {
      sessionID,
      agent: context?.agent,
      model: context?.model,
      variant: context?.variant,
      parts: [{ type: 'text', text: prompt }],
    };

    const flatResult = await promptAsync(flatPayload).catch((error: unknown) => ({ error }));
    if (!isFailureResponse(flatResult)) return;

    const legacyResult = await promptAsync({
      path: { id: sessionID },
      query: { directory: this.input.directory },
      body: {
        agent: context?.agent,
        model: context?.model,
        parts: [{ type: 'text', text: prompt }],
      },
    });

    if (isFailureResponse(legacyResult)) {
      const error = (legacyResult as { error?: unknown }).error;
      throw new Error(error instanceof Error ? error.message : String(error));
    }
  }

  private readSessionID(event: OpenCodeEvent): string | undefined {
    const direct = event.properties?.sessionID;
    if (typeof direct === 'string') return direct;

    const info = event.properties?.info;
    if (info && typeof info === 'object' && 'id' in info) {
      const id = (info as { id?: unknown }).id;
      if (typeof id === 'string') return id;
    }

    return undefined;
  }

  private formatContext(contexts: CanvasContextEvent[]): string {
    const sections = contexts.map((item) => {
      const header = item.label
        ? `Canvas context from ${item.sourceID} (${item.label})`
        : `Canvas context from ${item.sourceID}`;
      const data = compactJson(item.data);
      return [header, item.text, data ? `Data: ${data}` : undefined].filter(Boolean).join('\n');
    });

    return ['Additional context from interactive widgets:', ...sections].join('\n\n');
  }
}
