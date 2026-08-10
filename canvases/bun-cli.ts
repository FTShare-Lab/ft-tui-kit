import React from 'react';
import { render } from 'ink';
import path from 'path';
import { pathToFileURL } from 'url';
import type { RendererLaunchConfig } from '../src/canvas/protocol.ts';

export interface BunCanvasEntry {
  type: 'bun' | 'bun-ink';
  module: string;
  export?: string;
}

interface BunLaunchInput {
  kind: string;
  canvasDir: string;
  entry: BunCanvasEntry;
  launchFile: string;
}

interface BunStandaloneInput {
  kind: string;
  canvasDir: string;
  entry: BunCanvasEntry;
  id: string;
  scenario: string;
  config: unknown;
}

type CanvasComponent = React.ComponentType<{
  id: string;
  config?: unknown;
  launch?: RendererLaunchConfig;
  scenario?: string;
}>;

export async function runBunCanvasFromLaunch(input: BunLaunchInput): Promise<void> {
  const launch = await readJsonFile<RendererLaunchConfig>(input.launchFile);
  if (launch.kind !== input.kind) {
    throw new Error(`Launch file kind mismatch: expected ${input.kind}, got ${launch.kind}`);
  }

  const config = await readJsonFile<unknown>(launch.configPath);
  setWindowTitle(`canvas: ${launch.kind}`);

  await renderBunCanvas({
    canvasDir: input.canvasDir,
    entry: input.entry,
    id: launch.widgetId,
    config,
    launch,
    scenario: launch.scenario,
  });
}

export async function runBunCanvasStandalone(input: BunStandaloneInput): Promise<void> {
  setWindowTitle(`canvas: ${input.kind}`);

  await renderBunCanvas({
    canvasDir: input.canvasDir,
    entry: input.entry,
    id: input.id,
    config: input.config,
    scenario: input.scenario,
  });
}

async function renderBunCanvas(input: {
  canvasDir: string;
  entry: BunCanvasEntry;
  id: string;
  config?: unknown;
  launch?: RendererLaunchConfig;
  scenario?: string;
}): Promise<void> {
  clearScreen();
  installCursorRestore();

  const Component = await loadCanvasComponent(input.canvasDir, input.entry);
  const { waitUntilExit } = render(
    React.createElement(Component, {
      id: input.id,
      config: input.config,
      launch: input.launch,
      scenario: input.scenario,
    }),
    {
      exitOnCtrlC: true,
    }
  );

  await waitUntilExit();
}

async function loadCanvasComponent(
  canvasDir: string,
  entry: BunCanvasEntry
): Promise<CanvasComponent> {
  const modulePath = path.resolve(canvasDir, entry.module);
  const canvasModule = await import(pathToFileURL(modulePath).href);
  const exportName = entry.export ?? 'default';
  const component = canvasModule[exportName] ?? canvasModule.default;

  if (!component) {
    throw new Error(`Canvas module ${modulePath} does not export ${exportName}`);
  }

  return component as CanvasComponent;
}

async function readJsonFile<T>(file: string): Promise<T> {
  return (await Bun.file(file).json()) as T;
}

function setWindowTitle(title: string): void {
  process.stdout.write(`\x1b]0;${title}\x07`);
}

function clearScreen(): void {
  process.stdout.write('\x1b[2J\x1b[H\x1b[?25l');
}

let cursorRestoreInstalled = false;

function installCursorRestore(): void {
  if (cursorRestoreInstalled) return;
  cursorRestoreInstalled = true;

  process.on('exit', showCursor);
  process.on('SIGINT', () => {
    showCursor();
    process.exit();
  });
}

function showCursor(): void {
  process.stdout.write('\x1b[?25h');
}
