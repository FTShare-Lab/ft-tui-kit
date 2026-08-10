import { createHash } from 'node:crypto';
import {
  appendFileSync,
  chmodSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import type { CodexCanvasEvent } from './event-broker.ts';

const DEFAULT_EVENT_DIR = path.join(tmpdir(), 'ft-financial-canvas-v2', 'codex-events');
const MAX_JOURNAL_BYTES = 512 * 1024;
const MAX_ACTIVE_EVENTS = 100;
const EVENT_TTL_MS = 30 * 60 * 1000;

type JournalEntry =
  { operation: 'event'; event: CodexCanvasEvent } | { operation: 'ack'; eventID: string };

export class CodexEventStore {
  constructor(private readonly root = process.env.FT_CANVAS_EVENT_DIR ?? DEFAULT_EVENT_DIR) {}

  record(sessionID: string, event: CodexCanvasEvent): void {
    this.appendJournal(sessionID, { operation: 'event', event });
  }

  acknowledge(sessionID: string, eventID: string): void {
    this.appendJournal(sessionID, { operation: 'ack', eventID });
  }

  consume(sessionID: string): CodexCanvasEvent[] {
    const journal = this.journalPath(sessionID);
    const claimed = `${journal}.consume-${process.pid}-${Date.now().toString(36)}`;
    try {
      renameSync(journal, claimed);
    } catch (error) {
      if (isMissingFile(error)) return [];
      throw error;
    }

    try {
      const events = replayJournal(readFileSync(claimed, 'utf8'));
      const active = events
        .filter((event) => event.createdAt >= Date.now() - EVENT_TTL_MS)
        .slice(-MAX_ACTIVE_EVENTS);
      if (active.length)
        this.appendConsumedIDs(
          sessionID,
          active.map((event) => event.id)
        );
      return active;
    } finally {
      rmSync(claimed, { force: true });
    }
  }

  takeConsumedIDs(sessionID: string): Set<string> {
    const source = this.consumedPath(sessionID);
    const claimed = `${source}.consume-${process.pid}-${Date.now().toString(36)}`;
    try {
      renameSync(source, claimed);
    } catch (error) {
      if (isMissingFile(error)) return new Set();
      throw error;
    }

    try {
      return new Set(
        readFileSync(claimed, 'utf8')
          .split('\n')
          .map((line) => line.trim())
          .filter(Boolean)
      );
    } finally {
      rmSync(claimed, { force: true });
    }
  }

  clear(sessionID: string): void {
    rmSync(this.journalPath(sessionID), { force: true });
    rmSync(this.consumedPath(sessionID), { force: true });
  }

  private appendJournal(sessionID: string, entry: JournalEntry): void {
    this.ensureRoot();
    const journal = this.journalPath(sessionID);
    appendFileSync(journal, `${JSON.stringify(entry)}\n`, { encoding: 'utf8', mode: 0o600 });
    chmodSync(journal, 0o600);

    try {
      if (statSync(journal).size > MAX_JOURNAL_BYTES) this.compact(journal);
    } catch (error) {
      if (!isMissingFile(error)) throw error;
    }
  }

  private appendConsumedIDs(sessionID: string, eventIDs: string[]): void {
    this.ensureRoot();
    const consumed = this.consumedPath(sessionID);
    appendFileSync(consumed, `${eventIDs.join('\n')}\n`, { encoding: 'utf8', mode: 0o600 });
    chmodSync(consumed, 0o600);
  }

  private compact(journal: string): void {
    const claimed = `${journal}.compact-${process.pid}-${Date.now().toString(36)}`;
    try {
      renameSync(journal, claimed);
    } catch (error) {
      if (isMissingFile(error)) return;
      throw error;
    }

    try {
      const active = replayJournal(readFileSync(claimed, 'utf8'))
        .filter((event) => event.createdAt >= Date.now() - EVENT_TTL_MS)
        .slice(-MAX_ACTIVE_EVENTS);
      if (active.length) {
        const contents = active
          .map((event) => JSON.stringify({ operation: 'event', event } satisfies JournalEntry))
          .join('\n');
        writeFileSync(journal, `${contents}\n`, { encoding: 'utf8', mode: 0o600 });
      }
    } finally {
      rmSync(claimed, { force: true });
    }
  }

  private ensureRoot(): void {
    mkdirSync(this.root, { recursive: true, mode: 0o700 });
    chmodSync(this.root, 0o700);
  }

  private journalPath(sessionID: string): string {
    return path.join(this.root, `${sessionKey(sessionID)}.ndjson`);
  }

  private consumedPath(sessionID: string): string {
    return path.join(this.root, `${sessionKey(sessionID)}.consumed`);
  }
}

function replayJournal(contents: string): CodexCanvasEvent[] {
  const events = new Map<string, CodexCanvasEvent>();
  for (const line of contents.split('\n')) {
    if (!line.trim()) continue;
    try {
      const entry = JSON.parse(line) as JournalEntry;
      if (entry.operation === 'event' && isCanvasEvent(entry.event)) {
        events.set(entry.event.id, entry.event);
      } else if (entry.operation === 'ack' && typeof entry.eventID === 'string') {
        events.delete(entry.eventID);
      }
    } catch {
      // A partially written final line is ignored; earlier complete events remain available.
    }
  }
  return [...events.values()];
}

function isCanvasEvent(value: unknown): value is CodexCanvasEvent {
  if (!value || typeof value !== 'object') return false;
  const event = value as Partial<CodexCanvasEvent>;
  return (
    typeof event.id === 'string' &&
    (event.delivery === 'context' || event.delivery === 'action') &&
    typeof event.sourceID === 'string' &&
    typeof event.sourceEvent === 'string' &&
    typeof event.text === 'string' &&
    typeof event.createdAt === 'number'
  );
}

function sessionKey(sessionID: string): string {
  return createHash('sha256').update(sessionID).digest('hex').slice(0, 32);
}

function isMissingFile(error: unknown): boolean {
  return error instanceof Error && 'code' in error && error.code === 'ENOENT';
}
