import { fileURLToPath } from 'node:url';
import type { CanvasToolContext } from '../../canvas/manager/event-sink.ts';

const SANDBOX_STATE_META_KEY = 'codex/sandbox-state-meta';

export interface CodexRequestContext {
  _meta?: Record<string, unknown>;
  sessionId?: string;
}

export function canvasToolContextFromCodex(extra: CodexRequestContext): CanvasToolContext {
  const sessionID = readString(extra._meta?.threadId) ?? readString(extra.sessionId);
  if (!sessionID) {
    throw new Error(
      'Codex did not provide _meta.threadId for this MCP tool call. Update Codex or call the Canvas MCP server from a Codex thread.'
    );
  }

  return {
    sessionID,
    workspace: workspaceFromMeta(extra._meta) ?? process.cwd(),
  };
}

function workspaceFromMeta(meta: Record<string, unknown> | undefined): string | undefined {
  const sandboxState = meta?.[SANDBOX_STATE_META_KEY];
  if (!sandboxState || typeof sandboxState !== 'object') return undefined;

  const sandboxCwd = readString(
    (sandboxState as { sandboxCwd?: unknown; sandbox_cwd?: unknown }).sandboxCwd ??
      (sandboxState as { sandbox_cwd?: unknown }).sandbox_cwd
  );
  if (!sandboxCwd) return undefined;

  if (!sandboxCwd.startsWith('file:')) return sandboxCwd;

  try {
    return fileURLToPath(sandboxCwd);
  } catch {
    return undefined;
  }
}

function readString(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined;
}
