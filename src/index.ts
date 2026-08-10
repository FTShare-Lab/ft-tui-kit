/**
 * ft financial canvas plugin
 *
 * Terminal-native financial and supporting canvases for OpenCode:
 * - Candlesticks: inspect market candles and send chart context to the LLM
 * - Market table: inspect quote rankings and select securities
 * - News list: search and inspect market news
 * - Security snapshot: inspect a single security's market and valuation data
 * - Chart: inspect grouped statistical data and send selected cells to the LLM
 * - DAG: inspect data lineage, analysis flows, and other directed dependencies
 *
 * Requires: tmux session, Bun runtime
 */

import type { Plugin, ToolContext } from '@opencode-ai/plugin';
import { tool } from '@opencode-ai/plugin';
import path from 'path';
import { CanvasManager } from './canvas/manager/canvas-manager.ts';
import type { CanvasToolContext } from './canvas/manager/event-sink.ts';
import { createDefaultManifestRegistry } from './canvas/manifest.ts';
import { TmuxManager } from './canvas/manager/tmux.ts';
import { PromptBridge } from './hosts/opencode/prompt-bridge.ts';
import { SocialPostCoordinator } from './social-post/coordinator.ts';

interface CommandFrontmatter {
  description?: string;
  agent?: string;
  model?: string;
  subtask?: boolean;
}

interface ParsedCommand {
  name: string;
  frontmatter: CommandFrontmatter;
  template: string;
}

function parseFrontmatter(content: string): { frontmatter: CommandFrontmatter; body: string } {
  const frontmatterRegex = /^---\n([\s\S]*?)\n---\n([\s\S]*)$/;
  const match = content.match(frontmatterRegex);

  if (!match) {
    return { frontmatter: {}, body: content.trim() };
  }

  const [, yamlContent, body] = match;
  const frontmatter: CommandFrontmatter = {};

  for (const line of yamlContent.split('\n')) {
    const colonIndex = line.indexOf(':');
    if (colonIndex === -1) continue;

    const key = line.slice(0, colonIndex).trim();
    const value = line.slice(colonIndex + 1).trim();

    if (key === 'description') frontmatter.description = value;
    if (key === 'agent') frontmatter.agent = value;
    if (key === 'model') frontmatter.model = value;
    if (key === 'subtask') frontmatter.subtask = value === 'true';
  }

  return { frontmatter, body: body.trim() };
}

async function loadCommands(): Promise<ParsedCommand[]> {
  const commands: ParsedCommand[] = [];
  const commandDir = path.join(getPluginRoot(), 'src', 'command');
  const glob = new Bun.Glob('**/*.md');

  try {
    for await (const file of glob.scan({ cwd: commandDir, absolute: true })) {
      const content = await Bun.file(file).text();
      const { frontmatter, body } = parseFrontmatter(content);

      const relativePath = path.relative(commandDir, file);
      const name = relativePath.replace(/\.md$/, '').replaceAll(path.sep, '-');

      commands.push({
        name,
        frontmatter,
        template: body,
      });
    }
  } catch {
    // Command directory may not exist in all environments.
  }

  return commands;
}

async function hasSkills(skillRoot: string): Promise<boolean> {
  const glob = new Bun.Glob('**/SKILL.md');

  try {
    for await (const _file of glob.scan({ cwd: skillRoot, absolute: true })) {
      return true;
    }
  } catch {
    return false;
  }

  return false;
}

function getPluginRoot(): string {
  return path.join(import.meta.dir, '..');
}

function hash(value: string): string {
  return new Bun.CryptoHasher('sha256').update(value).digest('hex').slice(0, 16);
}

function stringifyResult(input: unknown): string {
  return JSON.stringify(input);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function canvasToolContext(context: ToolContext, workspace: string): CanvasToolContext {
  return {
    sessionID: context.sessionID,
    agent: context.agent,
    workspace,
  };
}

async function runTool(action: () => Promise<Record<string, unknown>>): Promise<string> {
  try {
    return stringifyResult(await action());
  } catch (error) {
    return stringifyResult({ success: false, error: errorMessage(error) });
  }
}

function sessionIDFromEvent(event: unknown): string | undefined {
  if (!event || typeof event !== 'object') return undefined;
  const properties =
    'properties' in event ? (event as { properties?: unknown }).properties : undefined;
  if (!properties || typeof properties !== 'object') return undefined;

  const direct =
    'sessionID' in properties ? (properties as { sessionID?: unknown }).sessionID : undefined;
  if (typeof direct === 'string') return direct;

  const info = 'info' in properties ? (properties as { info?: unknown }).info : undefined;
  if (info && typeof info === 'object' && 'id' in info) {
    const id = (info as { id?: unknown }).id;
    if (typeof id === 'string') return id;
  }

  return undefined;
}

export const CanvasPlugin: Plugin = async (input) => {
  const commands = await loadCommands();
  const pluginRoot = getPluginRoot();
  const skillRoot = path.join(pluginRoot, 'skills');
  const shouldRegisterSkillRoot = await hasSkills(skillRoot);
  const promptBridge = new PromptBridge(input);
  const manifests = createDefaultManifestRegistry(pluginRoot);
  let socialPostCoordinator: SocialPostCoordinator | undefined;
  const manager = new CanvasManager({
    pluginRoot,
    runtimeRoot: `/tmp/ft-financial-canvas-v2/${hash(input.directory)}`,
    eventSink: promptBridge,
    tmux: new TmuxManager(),
    manifests,
    onCommand: (event) => {
      if (!socialPostCoordinator) {
        throw new Error('Internal Canvas command handler is not ready');
      }
      return socialPostCoordinator.handleCommand(event);
    },
  });
  socialPostCoordinator = new SocialPostCoordinator({ input, manager });
  const rendererSummary = manifests
    .publicList()
    .map((manifest) => `- ${manifest.name}: ${manifest.description}`)
    .join('\n');

  const canvasSpawn = tool({
    description: `Spawn an interactive terminal widget in a tmux split pane.

Use this when the user benefits from an interactive renderer in the right-side tmux canvas.
The renderer runs as a child process and communicates with this host only through two sockets.
Renderer selections, context, and actions can be sent back into the current OpenCode session.

Available renderers:
${rendererSummary}

Waits for the renderer to accept or reject its initial configuration. A rejected configuration
returns { success: false, id, error, ... } and keeps that Canvas available for canvas_update.
Returns JSON with { success, id, kind, scenario, status, paneID, visible, launchFile, controlSocketPath, eventSocketPath, error? }.`,
    args: {
      kind: tool.schema
        .string()
        .describe(
          'Renderer name from canvas_renderers, such as candlesticks, market-table, chart, or dag'
        ),
      scenario: tool.schema
        .string()
        .optional()
        .describe(
          'Renderer scenario, such as kline, quotes, search, overview, bar-graph, or display'
        ),
      config: tool.schema
        .string()
        .optional()
        .describe('JSON configuration for the widget. Use {} when no config is needed.'),
      title: tool.schema.string().optional().describe('Short human-readable widget title'),
      activate: tool.schema
        .boolean()
        .optional()
        .describe('Whether to switch the right pane to this widget immediately. Defaults to true.'),
    },
    async execute(
      args: {
        kind: string;
        scenario?: string;
        config?: string;
        title?: string;
        activate?: boolean;
      },
      context: ToolContext
    ) {
      return runTool(() =>
        manager.spawn({
          kind: args.kind,
          scenario: args.scenario,
          config: args.config ?? '{}',
          title: args.title,
          activate: args.activate,
          context: canvasToolContext(context, input.directory),
        })
      );
    },
  });

  const canvasRenderers = tool({
    description: 'List available external canvas renderers and their scenarios/capabilities.',
    args: {},
    async execute() {
      return stringifyResult(manager.availableRenderers());
    },
  });

  const canvasUpdate = tool({
    description: `Send updated JSON configuration to a running widget.

Use this to update a widget that was previously spawned with canvas_spawn.
Waits for the renderer to apply or reject this specific update.
Returns JSON with { success, id, status, error? }.`,
    args: {
      id: tool.schema.string().describe('Widget instance ID returned from canvas_spawn'),
      config: tool.schema.string().describe('New JSON configuration for the widget'),
    },
    async execute(args: { id: string; config: string }, context: ToolContext) {
      return runTool(() =>
        manager.update(args.id, args.config, canvasToolContext(context, input.directory))
      );
    },
  });

  const canvasSelection = tool({
    description: `Get the current renderer selection from a running canvas.

Returns JSON with { success, id, selection }.`,
    args: {
      id: tool.schema.string().describe('Widget instance ID returned from canvas_spawn'),
    },
    async execute(args: { id: string }, context: ToolContext) {
      return runTool(() => manager.selection(args.id, canvasToolContext(context, input.directory)));
    },
  });

  const canvasContent = tool({
    description: `Get renderer content from a running canvas when the renderer supports content RPC.

Returns JSON with { success, id, content }.`,
    args: {
      id: tool.schema.string().describe('Widget instance ID returned from canvas_spawn'),
    },
    async execute(args: { id: string }, context: ToolContext) {
      return runTool(() => manager.content(args.id, canvasToolContext(context, input.directory)));
    },
  });

  const canvasState = tool({
    description: `Get renderer state from a running canvas.

State is renderer-defined and may include cursor, selected item, visible range, zoom, or other UI state.`,
    args: {
      id: tool.schema.string().describe('Widget instance ID returned from canvas_spawn'),
      key: tool.schema.string().optional().describe('Optional renderer state key to request'),
    },
    async execute(args: { id: string; key?: string }, context: ToolContext) {
      return runTool(() =>
        manager.state(args.id, canvasToolContext(context, input.directory), args.key)
      );
    },
  });

  const canvasList = tool({
    description: `List widgets attached to the current OpenCode session.

Returns JSON with { success, activeID, focusedID, visibleIDs, layout, widgets }.`,
    args: {},
    async execute(_args: {}, context: ToolContext) {
      return stringifyResult(manager.list(context.sessionID));
    },
  });

  const canvasSwitch = tool({
    description: `Switch the visible right-side tmux pane to a running widget.

The widget process is preserved while hidden and becomes visible through tmux pane swapping.`,
    args: {
      id: tool.schema.string().describe('Widget instance ID to show in the right pane'),
    },
    async execute(args: { id: string }, context: ToolContext) {
      return runTool(() => manager.switch(args.id, canvasToolContext(context, input.directory)));
    },
  });

  const canvasNext = tool({
    description: `Switch the visible right-side tmux pane to the next widget for this session.`,
    args: {},
    async execute(_args: {}, context: ToolContext) {
      return runTool(() => manager.next(context.sessionID));
    },
  });

  const canvasLayout = tool({
    description: `Arrange Canvas widgets from the current OpenCode session into a predefined tmux layout.

Use this when two to four canvases should be visible simultaneously for comparison. This tool
accepts Canvas IDs only and never accepts raw tmux commands, pane IDs, or shell arguments.
Layouts: single, columns, rows, main-left, main-right, main-top, main-bottom, grid.
Returns JSON with { success, layout, visibleIDs, focusedID, hiddenIDs, panes }.`,
    args: {
      layout: tool.schema
        .string()
        .describe(
          'Layout preset: single, columns, rows, main-left, main-right, main-top, main-bottom, or grid'
        ),
      ids: tool.schema
        .array(tool.schema.string())
        .describe('One to four Canvas IDs owned by the current OpenCode session, in layout order'),
      mainPercent: tool.schema
        .number()
        .optional()
        .describe('Main Canvas share for main-* layouts, from 40 to 80. Defaults to 60.'),
      focus: tool.schema
        .string()
        .optional()
        .describe('Canvas ID to receive keyboard focus; it must also appear in ids'),
    },
    async execute(
      args: { layout: string; ids: string[]; mainPercent?: number; focus?: string },
      context: ToolContext
    ) {
      return runTool(() =>
        manager.layout({
          layout: args.layout as
            | 'single'
            | 'columns'
            | 'rows'
            | 'main-left'
            | 'main-right'
            | 'main-top'
            | 'main-bottom'
            | 'grid',
          ids: args.ids,
          mainPercent: args.mainPercent,
          focus: args.focus,
          context: canvasToolContext(context, input.directory),
        })
      );
    },
  });

  const canvasClose = tool({
    description: 'Close a running widget instance',
    args: {
      id: tool.schema.string().describe('Widget instance ID'),
    },
    async execute(args: { id: string }, context: ToolContext) {
      return runTool(() => manager.close(args.id, canvasToolContext(context, input.directory)));
    },
  });

  return {
    tool: {
      canvas_spawn: canvasSpawn,
      canvas_renderers: canvasRenderers,
      canvas_update: canvasUpdate,
      canvas_selection: canvasSelection,
      canvas_content: canvasContent,
      canvas_state: canvasState,
      canvas_list: canvasList,
      canvas_switch: canvasSwitch,
      canvas_next: canvasNext,
      canvas_layout: canvasLayout,
      canvas_close: canvasClose,
    },

    async event({ event }) {
      const openCodeEvent = event as { type: string; properties?: Record<string, unknown> };
      socialPostCoordinator.onOpenCodeEvent(openCodeEvent);
      await promptBridge.onOpenCodeEvent(openCodeEvent);
      if (openCodeEvent.type !== 'session.deleted') return;

      const sessionID = sessionIDFromEvent(openCodeEvent);
      if (sessionID) {
        await manager.closeSession(sessionID);
      }
    },

    async 'chat.message'(chatInput, output) {
      socialPostCoordinator.onChatMessage(chatInput);
      await promptBridge.onChatMessage(
        chatInput,
        output as unknown as {
          message: {
            id?: string;
            agent?: string;
            model?: { providerID: string; modelID: string; variant?: string };
          };
          parts: Array<Record<string, unknown>>;
        }
      );
    },

    async config(config: Record<string, unknown>) {
      const commandRecord = (config.command ?? {}) as Record<string, unknown>;

      for (const cmd of commands) {
        commandRecord[cmd.name] = {
          template: cmd.template,
          description: cmd.frontmatter.description,
          agent: cmd.frontmatter.agent,
          model: cmd.frontmatter.model,
          subtask: cmd.frontmatter.subtask,
        };
      }

      config.command = commandRecord;

      if (!shouldRegisterSkillRoot) return;

      const skills = (config.skills ?? {}) as { paths?: string[]; urls?: string[] };
      const paths = skills.paths ?? [];
      if (!paths.includes(skillRoot)) {
        paths.push(skillRoot);
      }
      config.skills = { ...skills, paths };
    },

    async dispose() {
      socialPostCoordinator.dispose();
      await manager.dispose();
    },
  };
};

export default CanvasPlugin;
