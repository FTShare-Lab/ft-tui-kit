import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import { z } from 'zod';
import { CanvasManager } from '../../canvas/manager/canvas-manager.ts';
import type { CanvasToolContext } from '../../canvas/manager/event-sink.ts';
import { TmuxManager } from '../../canvas/manager/tmux.ts';
import { createDefaultManifestRegistry } from '../../canvas/manifest.ts';
import { canvasToolContextFromCodex, type CodexRequestContext } from './context.ts';
import { CodexEventBroker } from './event-broker.ts';
import { resolvePluginRoot } from './plugin-root.ts';

const SERVER_NAME = 'ft-financial-canvas';
const SERVER_VERSION = '0.1.2';

const idSchema = z.string().min(1).describe('Canvas instance ID returned by canvas_spawn');
const layoutSchema = z.enum([
  'single',
  'columns',
  'rows',
  'main-left',
  'main-right',
  'main-top',
  'main-bottom',
  'grid',
]);

function result(payload: Record<string, unknown>, isError = false): CallToolResult {
  return {
    content: [{ type: 'text', text: JSON.stringify(payload) }],
    structuredContent: payload,
    ...(isError ? { isError: true } : {}),
  };
}

async function runTool(
  action: () => Record<string, unknown> | Promise<Record<string, unknown>>
): Promise<CallToolResult> {
  try {
    return result(await action());
  } catch (error) {
    return result(
      {
        success: false,
        error: error instanceof Error ? error.message : String(error),
      },
      true
    );
  }
}

function requestContext(extra: CodexRequestContext, broker: CodexEventBroker): CanvasToolContext {
  const context = canvasToolContextFromCodex(extra);
  broker.rememberToolContext(context);
  return context;
}

function runtimeRoot(): string {
  const identity = `${process.pid}:${process.cwd()}`;
  const suffix = createHash('sha256').update(identity).digest('hex').slice(0, 16);
  return path.join(tmpdir(), 'ft-financial-canvas-v2', `codex-${suffix}`);
}

export function createCodexMcpServer(): {
  server: McpServer;
  manager: CanvasManager;
  broker: CodexEventBroker;
} {
  const pluginRoot = resolvePluginRoot();
  const broker = new CodexEventBroker();
  const manifests = createDefaultManifestRegistry(pluginRoot);
  const manager = new CanvasManager({
    pluginRoot,
    runtimeRoot: runtimeRoot(),
    eventSink: broker,
    tmux: new TmuxManager(),
    manifests,
  });
  const rendererSummary = manifests
    .publicList()
    .map((manifest) => `${manifest.name} (${manifest.scenarios.join(', ')})`)
    .join('; ');

  const server = new McpServer(
    { name: SERVER_NAME, version: SERVER_VERSION },
    {
      capabilities: {
        experimental: {
          'codex/sandbox-state-meta': {},
        },
      },
      instructions:
        'Use canvas_spawn to open terminal-native widgets inside tmux. Use canvas_wait while the user interacts so renderer context and actions return to the current Codex turn.',
    }
  );

  server.registerTool(
    'canvas_spawn',
    {
      title: 'Spawn Canvas',
      description: `Spawn an interactive terminal widget in a tmux split pane. Available renderers: ${rendererSummary}. Configuration must be JSON encoded as a string. The renderer is scoped to the current Codex thread.`,
      inputSchema: z.object({
        kind: z.string().min(1).describe('Renderer name from canvas_renderers'),
        scenario: z.string().min(1).optional().describe('Optional renderer scenario'),
        config: z
          .string()
          .optional()
          .describe('JSON configuration string; defaults to an empty object'),
        title: z.string().min(1).optional().describe('Short human-readable Canvas title'),
        activate: z.boolean().optional().describe('Show this Canvas immediately; defaults to true'),
      }),
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false },
    },
    (args, extra) => {
      const context = requestContext(extra, broker);
      return runTool(() =>
        manager.spawn({
          kind: args.kind,
          scenario: args.scenario,
          config: args.config ?? '{}',
          title: args.title,
          activate: args.activate,
          context,
        })
      );
    }
  );

  server.registerTool(
    'canvas_renderers',
    {
      title: 'List Canvas Renderers',
      description: 'List public Canvas renderers and their supported scenarios and capabilities.',
      inputSchema: z.object({}),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
    },
    () => runTool(() => manager.availableRenderers())
  );

  server.registerTool(
    'canvas_update',
    {
      title: 'Update Canvas',
      description:
        'Apply a new JSON configuration to a running Canvas and wait for the renderer to accept or reject it.',
      inputSchema: z.object({
        id: idSchema,
        config: z.string().describe('New JSON configuration string'),
      }),
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true },
    },
    (args, extra) => {
      const context = requestContext(extra, broker);
      return runTool(() => manager.update(args.id, args.config, context));
    }
  );

  server.registerTool(
    'canvas_selection',
    {
      title: 'Read Canvas Selection',
      description: 'Read the current selection from a running Canvas.',
      inputSchema: z.object({ id: idSchema }),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
    },
    (args, extra) => {
      const context = requestContext(extra, broker);
      return runTool(() => manager.selection(args.id, context));
    }
  );

  server.registerTool(
    'canvas_content',
    {
      title: 'Read Canvas Content',
      description: 'Read renderer content when the running Canvas supports content RPC.',
      inputSchema: z.object({ id: idSchema }),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
    },
    (args, extra) => {
      const context = requestContext(extra, broker);
      return runTool(() => manager.content(args.id, context));
    }
  );

  server.registerTool(
    'canvas_state',
    {
      title: 'Read Canvas State',
      description:
        'Read renderer-defined state such as the cursor, selected item, visible range, or zoom.',
      inputSchema: z.object({
        id: idSchema,
        key: z.string().min(1).optional().describe('Optional renderer state key'),
      }),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
    },
    (args, extra) => {
      const context = requestContext(extra, broker);
      return runTool(() => manager.state(args.id, context, args.key));
    }
  );

  server.registerTool(
    'canvas_list',
    {
      title: 'List Canvases',
      description: 'List Canvases owned by the current Codex thread and show their layout state.',
      inputSchema: z.object({}),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
    },
    (_args, extra) => {
      const context = requestContext(extra, broker);
      return runTool(() => manager.list(context.sessionID));
    }
  );

  server.registerTool(
    'canvas_switch',
    {
      title: 'Switch Canvas',
      description: 'Show a running Canvas in the right-side tmux pane without restarting it.',
      inputSchema: z.object({ id: idSchema }),
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true },
    },
    (args, extra) => {
      const context = requestContext(extra, broker);
      return runTool(() => manager.switch(args.id, context));
    }
  );

  server.registerTool(
    'canvas_next',
    {
      title: 'Next Canvas',
      description: 'Show the next Canvas owned by the current Codex thread.',
      inputSchema: z.object({}),
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false },
    },
    (_args, extra) => {
      const context = requestContext(extra, broker);
      return runTool(() => manager.next(context.sessionID));
    }
  );

  server.registerTool(
    'canvas_layout',
    {
      title: 'Arrange Canvases',
      description:
        'Arrange one to four Canvases from the current Codex thread in a safe predefined tmux layout.',
      inputSchema: z.object({
        layout: layoutSchema.describe('Predefined Canvas layout'),
        ids: z.array(idSchema).min(1).max(4).describe('Canvas IDs in layout order'),
        mainPercent: z
          .number()
          .min(40)
          .max(80)
          .optional()
          .describe('Main Canvas percentage for main-* layouts; defaults to 60'),
        focus: idSchema.optional().describe('Visible Canvas ID that receives keyboard focus'),
      }),
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true },
    },
    (args, extra) => {
      const context = requestContext(extra, broker);
      return runTool(() =>
        manager.layout({
          layout: args.layout,
          ids: args.ids,
          mainPercent: args.mainPercent,
          focus: args.focus,
          context,
        })
      );
    }
  );

  server.registerTool(
    'canvas_close',
    {
      title: 'Close Canvas',
      description: 'Close a running Canvas owned by the current Codex thread.',
      inputSchema: z.object({ id: idSchema }),
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    (args, extra) => {
      const context = requestContext(extra, broker);
      return runTool(() => manager.close(args.id, context));
    }
  );

  server.registerTool(
    'canvas_wait',
    {
      title: 'Wait for Canvas Interaction',
      description:
        'Wait for the user to interact with a Canvas. Returns renderer context or an action to continue in the current Codex turn. Use this after canvas_spawn when user input is expected.',
      inputSchema: z.object({
        id: idSchema.optional().describe('Optional Canvas ID; omit to wait for any Canvas'),
        timeout_ms: z
          .number()
          .int()
          .min(0)
          .max(55_000)
          .optional()
          .describe('Wait duration in milliseconds; defaults to 30000 and is capped at 55000'),
      }),
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: false },
    },
    async (args, extra) => {
      try {
        const context = requestContext(extra, broker);
        const waited = await broker.wait(context.sessionID, {
          sourceID: args.id,
          timeoutMs: args.timeout_ms,
          signal: extra.signal,
        });
        return result({
          success: true,
          timedOut: waited.reason === 'timeout',
          cancelled: waited.reason === 'cancelled',
          reason: waited.reason,
          event: waited.event ?? null,
        });
      } catch (error) {
        return result(
          { success: false, error: error instanceof Error ? error.message : String(error) },
          true
        );
      }
    }
  );

  return { server, manager, broker };
}

export async function startCodexMcpServer(): Promise<void> {
  const { server, manager, broker } = createCodexMcpServer();
  const transport = new StdioServerTransport();
  let disposePromise: Promise<void> | undefined;
  const dispose = (): Promise<void> => {
    disposePromise ??= (async () => {
      broker.dispose();
      await manager.dispose();
    })();
    return disposePromise;
  };

  server.server.onclose = () => {
    void dispose();
  };

  const shutdown = (): void => {
    void dispose()
      .then(() => server.close())
      .finally(() => {
        process.exitCode = 0;
      });
  };
  process.once('SIGINT', shutdown);
  process.once('SIGTERM', shutdown);

  await server.connect(transport);
}

if (import.meta.main) {
  startCodexMcpServer().catch((error: unknown) => {
    const message = error instanceof Error ? (error.stack ?? error.message) : String(error);
    process.stderr.write(`Financial Canvas MCP server failed: ${message}\n`);
    process.exitCode = 1;
  });
}
