import { CodexEventStore } from './event-store.ts';
import type { CodexCanvasEvent } from './event-broker.ts';

const MAX_DATA_CHARS = 4_000;
const MAX_CONTEXT_CHARS = 24_000;

interface UserPromptSubmitInput {
  session_id?: unknown;
  hook_event_name?: unknown;
}

interface UserPromptSubmitOutput {
  hookSpecificOutput: {
    hookEventName: 'UserPromptSubmit';
    additionalContext: string;
  };
}

export function userPromptSubmitOutput(
  input: UserPromptSubmitInput,
  store = new CodexEventStore()
): UserPromptSubmitOutput | undefined {
  if (input.hook_event_name !== 'UserPromptSubmit' || typeof input.session_id !== 'string') {
    return undefined;
  }

  const events = store.consume(input.session_id);
  if (!events.length) return undefined;

  return {
    hookSpecificOutput: {
      hookEventName: 'UserPromptSubmit',
      additionalContext: formatEvents(events),
    },
  };
}

export async function startCodexHook(): Promise<void> {
  const rawInput = await readStandardInput();
  if (!rawInput.trim()) return;

  const input = JSON.parse(rawInput) as UserPromptSubmitInput;
  const output = userPromptSubmitOutput(input);
  if (output) process.stdout.write(`${JSON.stringify(output)}\n`);
}

function formatEvents(events: CodexCanvasEvent[]): string {
  const sections = events.map((event) => {
    const kind = event.delivery === 'action' ? 'action' : 'context';
    const header = event.label
      ? `Canvas ${kind} from ${event.sourceID} (${event.label})`
      : `Canvas ${kind} from ${event.sourceID}`;
    const data = compactData(event.data);
    return [header, event.text, data ? `Data: ${data}` : undefined].filter(Boolean).join('\n');
  });
  const full = ['Pending interactions from terminal Canvas widgets:', ...sections].join('\n\n');
  if (full.length <= MAX_CONTEXT_CHARS) return full;
  return `${full.slice(0, MAX_CONTEXT_CHARS)}\n\n[Additional Canvas context truncated]`;
}

function compactData(value: unknown): string | undefined {
  if (value === undefined) return undefined;
  try {
    const data = JSON.stringify(value);
    return data.length > MAX_DATA_CHARS ? `${data.slice(0, MAX_DATA_CHARS)}…` : data;
  } catch {
    return String(value).slice(0, MAX_DATA_CHARS);
  }
}

async function readStandardInput(): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of process.stdin) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  }
  return Buffer.concat(chunks).toString('utf8');
}

if (import.meta.main) {
  startCodexHook().catch((error: unknown) => {
    const message = error instanceof Error ? (error.stack ?? error.message) : String(error);
    process.stderr.write(`Financial Canvas hook failed open: ${message}\n`);
    process.exitCode = 0;
  });
}
