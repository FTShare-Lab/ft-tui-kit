import path from 'path';
import { existsSync, readdirSync, readFileSync, statSync } from 'fs';
import type { RendererCapabilities } from './protocol.ts';
import { shellQuote } from './manager/tmux.ts';

export interface CanvasManifest {
  name: string;
  description: string;
  defaultScenario: string;
  scenarios: string[];
  command: string[];
  capabilities: RendererCapabilities;
  internalOnly: boolean;
}

export interface RendererCommandInput {
  pluginRoot: string;
  launchFile: string;
  widgetId: string;
  kind: string;
  scenario: string;
  runtimeDir: string;
}

interface CanvasConfigFile {
  name: string;
  description?: string;
  defaultScenario: string;
  scenarios: string[];
  capabilities?: RendererCapabilities;
  internalOnly?: boolean;
  entry?: unknown;
}

export class CanvasManifestRegistry {
  private readonly manifests = new Map<string, CanvasManifest>();

  constructor(manifests: CanvasManifest[] = []) {
    for (const manifest of manifests) {
      this.register(manifest);
    }
  }

  register(manifest: CanvasManifest): void {
    if (!manifest.name.trim()) {
      throw new Error('Canvas manifest name is required');
    }
    if (!manifest.command.length) {
      throw new Error(`Canvas manifest ${manifest.name} must define a command`);
    }
    this.manifests.set(manifest.name, manifest);
  }

  require(name: string): CanvasManifest {
    const manifest = this.manifests.get(name);
    if (!manifest) {
      throw new Error(`Unknown canvas renderer: ${name}`);
    }
    return manifest;
  }

  list(): CanvasManifest[] {
    return [...this.manifests.values()];
  }

  publicList(): CanvasManifest[] {
    return this.list().filter((manifest) => !manifest.internalOnly);
  }
}

export function createDefaultManifestRegistry(pluginRoot: string): CanvasManifestRegistry {
  return new CanvasManifestRegistry(loadCanvasManifests(pluginRoot));
}

export function loadCanvasManifests(pluginRoot: string): CanvasManifest[] {
  const canvasesRoot = path.join(pluginRoot, 'canvases');
  if (!existsSync(canvasesRoot)) return [];

  return readdirSync(canvasesRoot)
    .map((entry) => path.join(canvasesRoot, entry))
    .filter((canvasDir) => statSync(canvasDir).isDirectory())
    .filter((canvasDir) => existsSync(path.join(canvasDir, 'config.json')))
    .map((canvasDir) => manifestFromConfig(canvasDir));
}

function manifestFromConfig(canvasDir: string): CanvasManifest {
  const configPath = path.join(canvasDir, 'config.json');
  const config = JSON.parse(readFileSync(configPath, 'utf8')) as CanvasConfigFile;
  const folderName = path.basename(canvasDir);

  validateCanvasConfig(config, folderName, configPath);

  return {
    name: config.name,
    description: config.description ?? '',
    defaultScenario: config.defaultScenario,
    scenarios: config.scenarios,
    capabilities: config.capabilities ?? {},
    internalOnly: config.internalOnly ?? false,
    command: [
      'bun',
      'run',
      '{pluginRoot}/canvases/launcher.ts',
      'renderer',
      '{kind}',
      '--launch-file',
      '{launchFile}',
    ],
  };
}

function validateCanvasConfig(
  config: CanvasConfigFile,
  folderName: string,
  configPath: string
): void {
  if (!config || typeof config !== 'object') {
    throw new Error(`Canvas config must be an object: ${configPath}`);
  }
  if (config.name !== folderName) {
    throw new Error(
      `Canvas config name mismatch in ${configPath}: expected ${folderName}, got ${config.name}`
    );
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

export function buildRendererCommand(
  manifest: CanvasManifest,
  input: RendererCommandInput
): string {
  const placeholders: Record<string, string> = {
    pluginRoot: input.pluginRoot,
    launchFile: input.launchFile,
    widgetId: input.widgetId,
    kind: input.kind,
    scenario: input.scenario,
    runtimeDir: input.runtimeDir,
  };

  return manifest.command
    .map((part) => replacePlaceholders(part, placeholders))
    .map((part) => normalizeCommandPart(part))
    .map(shellQuote)
    .join(' ');
}

function replacePlaceholders(value: string, placeholders: Record<string, string>): string {
  return value.replace(/\{([a-zA-Z0-9_]+)\}/g, (_match, key: string) => placeholders[key] ?? '');
}

function normalizeCommandPart(value: string): string {
  if (!value.includes('/')) return value;
  return path.normalize(value);
}
