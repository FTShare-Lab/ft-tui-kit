#!/usr/bin/env bun

import { existsSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

import { Command } from 'commander';

const CODEX_MARKETPLACE = 'personal';
const OPENCODE_CONFIG_NAME = 'opencode.json';
const CANVAS_PERMISSION = 'canvas_*';

interface PackageManifest extends Record<string, unknown> {
  name: string;
  version: string;
}

const packageRoot = realpathSync(path.resolve(import.meta.dir, '..'));
const packageManifest = readPackageManifest(path.join(packageRoot, 'package.json'));

const program = new Command()
  .name('ft-tui-kit')
  .description('Install ft-tui-kit for OpenCode or Codex')
  .version(packageManifest.version)
  .showHelpAfterError();

program
  .command('install <host>')
  .description('Install the integration in the current project')
  .action(async (host: string) => {
    if (host === 'opencode') {
      installOpenCode();
      return;
    }
    if (host === 'codex') {
      await installCodex();
      return;
    }
    throw new Error(`unsupported host: ${host}`);
  });

program
  .command('uninstall <host>')
  .description('Uninstall the Codex integration')
  .action(async (host: string) => {
    if (host !== 'codex') throw new Error(`unsupported host: ${host}`);
    await uninstallCodex();
  });

function installOpenCode(): void {
  const configPath = path.join(process.cwd(), OPENCODE_CONFIG_NAME);
  const config = existsSync(configPath) ? readJsonObject(configPath, 'OpenCode config') : {};
  const pluginSpecifier = pathToFileURL(path.join(packageRoot, 'src', 'index.ts')).href;

  if (config.plugin === undefined) config.plugin = [];
  if (!Array.isArray(config.plugin)) {
    throw new Error(`${configPath} field 'plugin' must be an array`);
  }
  if (!config.plugin.some((entry) => isConfiguredPlugin(entry, pluginSpecifier))) {
    config.plugin.push(pluginSpecifier);
  }

  if (typeof config.permission === 'string') {
    config.permission = { '*': config.permission };
  } else if (config.permission === undefined) {
    config.permission = {};
  }
  if (!isObject(config.permission)) {
    throw new Error(`${configPath} field 'permission' must be a string or object`);
  }
  config.permission[CANVAS_PERMISSION] = 'allow';

  writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`, 'utf8');
  writeLine(`Updated OpenCode configuration: ${configPath}`);
}

async function installCodex(): Promise<void> {
  await runCommand(process.execPath, [
    path.join(packageRoot, 'scripts', 'install-codex-plugin.ts'),
  ]);
}

async function uninstallCodex(): Promise<void> {
  const codex = Bun.which('codex');
  if (!codex) throw new Error('codex was not found on PATH');
  await runCommand(codex, ['plugin', 'remove', `${packageManifest.name}@${CODEX_MARKETPLACE}`]);
}

function isConfiguredPlugin(entry: unknown, pluginSpecifier: string): boolean {
  const configured =
    typeof entry === 'string'
      ? entry
      : Array.isArray(entry) && typeof entry[0] === 'string'
        ? entry[0]
        : undefined;
  if (!configured) return false;
  return (
    configured === pluginSpecifier ||
    configured === packageManifest.name ||
    configured.startsWith(`${packageManifest.name}@`)
  );
}

function readPackageManifest(manifestPath: string): PackageManifest {
  const manifest = readJsonObject(manifestPath, 'package manifest');
  if (typeof manifest.name !== 'string' || !manifest.name.trim()) {
    throw new Error(`${manifestPath} must contain a package name`);
  }
  if (typeof manifest.version !== 'string' || !manifest.version.trim()) {
    throw new Error(`${manifestPath} must contain a package version`);
  }
  return manifest as PackageManifest;
}

function readJsonObject(filePath: string, label: string): Record<string, unknown> {
  let value: unknown;
  try {
    value = JSON.parse(readFileSync(filePath, 'utf8'));
  } catch (error) {
    throw new Error(`unable to read ${label} ${filePath}: ${errorMessage(error)}`);
  }
  if (!isObject(value)) throw new Error(`${label} ${filePath} must contain a JSON object`);
  return value;
}

async function runCommand(command: string, args: string[]): Promise<void> {
  writeLine(`> ${[command, ...args].join(' ')}`);
  const child = Bun.spawn([command, ...args], {
    cwd: process.cwd(),
    stdin: 'inherit',
    stdout: 'inherit',
    stderr: 'inherit',
  });
  const exitCode = await child.exited;
  if (exitCode !== 0) throw new Error(`${path.basename(command)} exited with code ${exitCode}`);
}

function isObject(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function writeLine(value: string): void {
  process.stdout.write(`${value}\n`);
}

program.parseAsync(process.argv).catch((error: unknown) => {
  process.stderr.write(`ft-tui-kit failed: ${errorMessage(error)}\n`);
  process.exitCode = 1;
});
