#!/usr/bin/env bun
import { program } from 'commander';
import path from 'path';
import { runBunCanvasFromLaunch, runBunCanvasStandalone, type BunCanvasEntry } from './bun-cli.ts';
import type { RendererLaunchConfig } from '../src/canvas/protocol.ts';

type CanvasEntry = BunCanvasEntry | CommandCanvasEntry;

interface CommandCanvasEntry {
  type: 'command';
  command: string[];
  showCommand?: string[];
}

interface CanvasConfigFile {
  schemaVersion?: number;
  name: string;
  description?: string;
  defaultScenario: string;
  scenarios: string[];
  internalOnly?: boolean;
  entry: CanvasEntry;
}

interface StandaloneOptions {
  config?: string;
  configFile?: string;
  scenario?: string;
  id?: string;
}

const canvasesRoot = import.meta.dir;
const pluginRoot = path.resolve(canvasesRoot, '..');

async function renderer(kind: string, launchFile: string): Promise<void> {
  const canvas = await loadCanvas(kind);
  const launch = await readJsonFile<RendererLaunchConfig>(launchFile);

  if (launch.kind !== canvas.config.name) {
    throw new Error(
      `Launch file kind mismatch: expected ${canvas.config.name}, got ${launch.kind}`
    );
  }

  if (isBunEntry(canvas.config.entry)) {
    await runBunCanvasFromLaunch({
      kind: canvas.config.name,
      canvasDir: canvas.dir,
      entry: canvas.config.entry,
      launchFile,
    });
    return;
  }

  await runCommand(
    fillCommand(canvas.config.entry.command, {
      canvasDir: canvas.dir,
      kind: canvas.config.name,
      launchFile,
      pluginRoot,
      runtimeDir: launch.runtimeDir,
      scenario: launch.scenario,
    })
  );
}

async function show(kind: string, options: StandaloneOptions): Promise<void> {
  const canvas = await loadCanvas(kind);
  if (canvas.config.internalOnly) {
    throw new Error(`Canvas ${canvas.config.name} is plugin-internal and cannot run standalone`);
  }
  const scenario = options.scenario ?? canvas.config.defaultScenario;

  if (isBunEntry(canvas.config.entry)) {
    await runBunCanvasStandalone({
      kind: canvas.config.name,
      canvasDir: canvas.dir,
      entry: canvas.config.entry,
      id: options.id ?? `${canvas.config.name}-standalone`,
      scenario,
      config: await readStandaloneConfig(options),
    });
    return;
  }

  if (!canvas.config.entry.showCommand?.length) {
    throw new Error(
      `Canvas ${canvas.config.name} does not define entry.showCommand for standalone show`
    );
  }

  await runCommand(
    fillCommand(canvas.config.entry.showCommand, {
      canvasDir: canvas.dir,
      kind: canvas.config.name,
      launchFile: '',
      pluginRoot,
      runtimeDir: '',
      scenario,
    })
  );
}

async function loadCanvas(kind: string): Promise<{ dir: string; config: CanvasConfigFile }> {
  const dir = path.join(canvasesRoot, kind);
  const config = await readJsonFile<CanvasConfigFile>(path.join(dir, 'config.json'));
  validateCanvasConfig(config, kind);
  return { dir, config };
}

async function readStandaloneConfig(options: StandaloneOptions): Promise<unknown> {
  const configText =
    options.config ?? (options.configFile ? await Bun.file(options.configFile).text() : undefined);
  return configText ? JSON.parse(configText) : undefined;
}

async function readJsonFile<T>(file: string): Promise<T> {
  return (await Bun.file(file).json()) as T;
}

function validateCanvasConfig(config: CanvasConfigFile, folderName: string): void {
  if (!config || typeof config !== 'object') {
    throw new Error(`Canvas ${folderName} config.json must be an object`);
  }
  if (config.name !== folderName) {
    throw new Error(`Canvas config name mismatch: folder ${folderName}, config ${config.name}`);
  }
  if (!config.defaultScenario) {
    throw new Error(`Canvas ${config.name} must define defaultScenario`);
  }
  if (!Array.isArray(config.scenarios) || !config.scenarios.length) {
    throw new Error(`Canvas ${config.name} must define at least one scenario`);
  }
  if (!config.scenarios.includes(config.defaultScenario)) {
    throw new Error(`Canvas ${config.name} defaultScenario must be listed in scenarios`);
  }
  if (config.internalOnly !== undefined && typeof config.internalOnly !== 'boolean') {
    throw new Error(`Canvas ${config.name} internalOnly must be a boolean`);
  }
  if (!config.entry || typeof config.entry !== 'object') {
    throw new Error(`Canvas ${config.name} must define entry`);
  }
}

function isBunEntry(entry: CanvasEntry): entry is BunCanvasEntry {
  return entry.type === 'bun' || entry.type === 'bun-ink';
}

function fillCommand(command: string[], placeholders: Record<string, string>): string[] {
  return command.map((part) =>
    path.normalize(
      part.replace(/\{([a-zA-Z0-9_]+)\}/g, (_match, key: string) => placeholders[key] ?? '')
    )
  );
}

async function runCommand(command: string[]): Promise<void> {
  if (!command.length) {
    throw new Error('Canvas command entry cannot be empty');
  }

  const subprocess = Bun.spawn(command, {
    stdin: 'inherit',
    stdout: 'inherit',
    stderr: 'inherit',
  });
  const code = await subprocess.exited;
  if (code !== 0) {
    process.exit(code);
  }
}

program
  .name('ft-financial-canvas')
  .description('Renderer launcher for ft financial canvas')
  .version('2.0.0');

program
  .command('renderer <kind>')
  .description('Run a canvas renderer from a host-generated launch file')
  .requiredOption('--launch-file <path>', 'Renderer launch file generated by ft financial canvas')
  .action(async (kind: string, options: { launchFile: string }) => {
    await renderer(kind, options.launchFile);
  });

program
  .command('show <kind>')
  .description('Run a canvas directly without host IPC, for local visual development')
  .option('--id <id>', 'Canvas ID')
  .option('--config <json>', 'Canvas configuration JSON')
  .option('--config-file <path>', 'Canvas configuration file')
  .option('--scenario <name>', 'Scenario name')
  .action(async (kind: string, options: StandaloneOptions) => {
    await show(kind, options);
  });

program.parse();
