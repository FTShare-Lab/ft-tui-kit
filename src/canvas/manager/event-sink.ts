export interface CanvasToolContext {
  sessionID: string;
  agent?: string;
  workspace?: string;
}

export type CanvasSourceEvent = 'selection' | 'context' | 'action' | 'artifact';

export interface CanvasContextEvent {
  sourceID: string;
  sourceEvent: Exclude<CanvasSourceEvent, 'action'>;
  label?: string;
  text: string;
  data?: unknown;
  createdAt: number;
}

export interface CanvasActionEvent {
  sourceID: string;
  sourceEvent: CanvasSourceEvent;
  label?: string;
  prompt: string;
  createdAt: number;
}

export interface CanvasEventSink {
  rememberToolContext(context: CanvasToolContext): void;
  attachContext(sessionID: string, context: CanvasContextEvent): void;
  enqueueAction(sessionID: string, action: CanvasActionEvent): Promise<void>;
  clear(sessionID: string): void;
}
