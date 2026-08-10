#!/usr/bin/env bun

import {
  chmodSync,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  type Stats,
  writeFileSync,
} from 'node:fs';
import { homedir } from 'node:os';
import path from 'node:path';

const DEFAULT_MARKETPLACE_PATH = path.join(homedir(), '.agents', 'plugins', 'marketplace.json');
const DEFAULT_MARKETPLACE_NAME = 'personal';
const DEFAULT_CATEGORY = 'Productivity';
const SOURCE_MARKER = '.ft-financial-canvas-source.json';
const IGNORED_SOURCE_DIRECTORIES = new Set([
  '.git',
  '.idea',
  '.memory',
  '.vscode',
  'coverage',
  'node_modules',
  'target',
]);
const VALID_INSTALL_POLICIES = new Set(['NOT_AVAILABLE', 'AVAILABLE', 'INSTALLED_BY_DEFAULT']);
const VALID_AUTH_POLICIES = new Set(['ON_INSTALL', 'ON_USE']);

interface InstallOptions {
  marketplacePath: string;
  marketplaceName?: string;
  cachebuster?: string;
  build: boolean;
  useCachebuster: boolean;
  force: boolean;
  dryRun: boolean;
}

interface PluginManifest extends Record<string, unknown> {
  name: string;
  version: string;
}

interface MarketplaceEntry extends Record<string, unknown> {
  name: string;
  source: {
    source: 'local';
    path: string;
  };
  policy: Record<string, unknown> & {
    installation: string;
    authentication: string;
  };
  category: string;
}

interface MarketplaceDocument extends Record<string, unknown> {
  name: string;
  plugins: unknown[];
}

interface MarketplaceUpdate {
  document: MarketplaceDocument;
  marketplaceName: string;
  entryExisted: boolean;
  changed: boolean;
}

type SourcePlan = 'create' | 'update' | 'replace';

async function main(): Promise<void> {
  const options = parseArgs(process.argv.slice(2));
  const pluginRoot = realpathSync(path.resolve(import.meta.dir, '..'));
  const manifestPath = path.join(pluginRoot, '.codex-plugin', 'plugin.json');
  const manifest = readPluginManifest(manifestPath);
  const marketplacePath = path.resolve(expandHome(options.marketplacePath));
  const marketplaceRoot = marketplaceRootFromManifest(marketplacePath);
  const sourcePath = path.join(marketplaceRoot, 'plugins', manifest.name);
  const sourceRelativePath = `./plugins/${manifest.name}`;
  const category = readPluginCategory(manifest);
  const marketplace = prepareMarketplace(
    marketplacePath,
    options.marketplaceName,
    manifest.name,
    sourceRelativePath,
    category,
    options.force
  );
  const sourcePlan = planPluginSource(sourcePath, pluginRoot, manifest.name, options.force);
  const isDefaultMarketplace = marketplacePath === path.resolve(DEFAULT_MARKETPLACE_PATH);
  const shouldCachebust =
    options.useCachebuster && (marketplace.entryExisted || options.cachebuster !== undefined);

  if (options.dryRun) {
    printPlan({
      pluginRoot,
      marketplacePath,
      marketplaceRoot,
      marketplaceName: marketplace.marketplaceName,
      sourcePath,
      sourcePlan,
      shouldBuild: options.build,
      shouldCachebust,
      shouldRegisterMarketplace: !isDefaultMarketplace,
    });
    return;
  }

  const codex = Bun.which('codex');
  if (!codex) throw new Error('codex was not found on PATH');

  if (options.build) {
    await runCommand(process.execPath, ['run', 'build:plugin'], pluginRoot);
    assertBuildOutputs(pluginRoot);
  }

  if (shouldCachebust) {
    const nextVersion = withCachebuster(
      manifest.version,
      options.cachebuster ?? defaultCachebuster()
    );
    if (nextVersion !== manifest.version) {
      manifest.version = nextVersion;
      writeJsonAtomic(manifestPath, manifest);
      writeLine(`Updated plugin cachebuster: ${nextVersion}`);
    }
  }

  await syncPluginSource(sourcePlan, sourcePath, pluginRoot, manifest.name);

  if (marketplace.changed || !existsSync(marketplacePath)) {
    writeJsonAtomic(marketplacePath, marketplace.document);
    writeLine(`Updated marketplace: ${marketplacePath}`);
  } else {
    writeLine(`Marketplace entry is up to date: ${marketplacePath}`);
  }

  if (!isDefaultMarketplace) {
    await runCommand(codex, ['plugin', 'marketplace', 'add', marketplaceRoot], pluginRoot);
  }

  await runCommand(
    codex,
    ['plugin', 'add', `${manifest.name}@${marketplace.marketplaceName}`],
    pluginRoot
  );

  writeLine('Codex plugin installation completed. Start a new thread to load the updated plugin.');
  writeLine(`Plugin source snapshot: ${sourcePath} (from ${pluginRoot})`);
}

function parseArgs(args: string[]): InstallOptions {
  const options: InstallOptions = {
    marketplacePath: DEFAULT_MARKETPLACE_PATH,
    build: true,
    useCachebuster: true,
    force: false,
    dryRun: false,
  };

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === '--') continue;
    if (argument === '--help' || argument === '-h') {
      writeLine(usage());
      process.exit(0);
    }
    if (argument === '--marketplace-path') {
      options.marketplacePath = requireValue(args, ++index, argument);
      continue;
    }
    if (argument === '--marketplace-name') {
      options.marketplaceName = requireValue(args, ++index, argument);
      continue;
    }
    if (argument === '--cachebuster') {
      options.cachebuster = requireValue(args, ++index, argument);
      continue;
    }
    if (argument === '--no-cachebuster') {
      options.useCachebuster = false;
      continue;
    }
    if (argument === '--no-build') {
      options.build = false;
      continue;
    }
    if (argument === '--force') {
      options.force = true;
      continue;
    }
    if (argument === '--dry-run') {
      options.dryRun = true;
      continue;
    }
    throw new Error(`unknown argument: ${argument}\n\n${usage()}`);
  }

  if (!options.useCachebuster && options.cachebuster) {
    throw new Error('--cachebuster cannot be combined with --no-cachebuster');
  }
  if (options.marketplaceName && !/^[A-Za-z0-9_-]+$/.test(options.marketplaceName)) {
    throw new Error('marketplace name may contain only letters, digits, `_`, and `-`');
  }
  return options;
}

function usage(): string {
  return `Install ft-financial-canvas into a local Codex marketplace.

Usage:
  bun run codex:install [-- options]
  ./scripts/install-codex-plugin.ts [options]

Options:
  --marketplace-path <path>  marketplace.json path (default: ~/.agents/plugins/marketplace.json)
  --marketplace-name <name>  name for a new non-default marketplace
  --cachebuster <token>      explicit Codex cachebuster for reinstall
  --no-cachebuster           do not update the manifest version on reinstall
  --no-build                 skip bun run build:plugin
  --force                    replace a conflicting entry or source directory
  --dry-run                  validate and print the plan without changing external state
  -h, --help                 show this help`;
}

function requireValue(args: string[], index: number, option: string): string {
  const value = args[index];
  if (!value || value.startsWith('--')) throw new Error(`${option} requires a value`);
  return value;
}

function readPluginManifest(manifestPath: string): PluginManifest {
  const manifest = readJsonObject(manifestPath, 'plugin manifest');
  if (typeof manifest.name !== 'string' || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(manifest.name)) {
    throw new Error(`${manifestPath} must contain a normalized plugin name`);
  }
  if (typeof manifest.version !== 'string' || !manifest.version.trim()) {
    throw new Error(`${manifestPath} must contain a non-empty version`);
  }
  return manifest as PluginManifest;
}

function readPluginCategory(manifest: PluginManifest): string {
  const pluginInterface = manifest.interface;
  if (isObject(pluginInterface) && typeof pluginInterface.category === 'string') {
    return pluginInterface.category;
  }
  return DEFAULT_CATEGORY;
}

function marketplaceRootFromManifest(marketplacePath: string): string {
  const marketplaceDirectory = path.dirname(marketplacePath);
  const agentsDirectory = path.dirname(marketplaceDirectory);
  if (
    path.basename(marketplacePath) !== 'marketplace.json' ||
    path.basename(marketplaceDirectory) !== 'plugins' ||
    path.basename(agentsDirectory) !== '.agents'
  ) {
    throw new Error(
      'marketplace path must use <marketplace-root>/.agents/plugins/marketplace.json'
    );
  }
  return path.dirname(agentsDirectory);
}

function prepareMarketplace(
  marketplacePath: string,
  requestedName: string | undefined,
  pluginName: string,
  sourcePath: string,
  category: string,
  force: boolean
): MarketplaceUpdate {
  const exists = existsSync(marketplacePath);
  const original = exists ? readJsonObject(marketplacePath, 'marketplace') : undefined;
  const originalSnapshot = original ? JSON.stringify(original) : undefined;
  if (!original && marketplacePath !== path.resolve(DEFAULT_MARKETPLACE_PATH) && !requestedName) {
    throw new Error('a new non-default marketplace requires --marketplace-name');
  }

  const document: MarketplaceDocument = original
    ? validateMarketplaceDocument(original, marketplacePath)
    : {
        name: requestedName ?? DEFAULT_MARKETPLACE_NAME,
        interface: { displayName: displayName(requestedName ?? DEFAULT_MARKETPLACE_NAME) },
        plugins: [],
      };

  if (requestedName && document.name !== requestedName) {
    throw new Error(
      `${marketplacePath} already uses marketplace name '${document.name}', not '${requestedName}'`
    );
  }

  const entryIndex = document.plugins.findIndex(
    (entry) => isObject(entry) && entry.name === pluginName
  );
  const entry = marketplaceEntry(pluginName, sourcePath, category);
  if (entryIndex >= 0) {
    const existingEntry = document.plugins[entryIndex];
    if (!isObject(existingEntry)) throw new Error(`invalid marketplace entry: ${pluginName}`);
    const existingSource = existingEntry.source;
    const sourceMatches =
      isObject(existingSource) &&
      existingSource.source === 'local' &&
      existingSource.path === sourcePath;
    if (!sourceMatches && !force) {
      throw new Error(
        `marketplace entry '${pluginName}' points elsewhere; rerun with --force to replace it`
      );
    }

    const existingPolicy = isObject(existingEntry.policy) ? existingEntry.policy : {};
    const installation =
      typeof existingPolicy.installation === 'string' &&
      VALID_INSTALL_POLICIES.has(existingPolicy.installation)
        ? existingPolicy.installation
        : entry.policy.installation;
    const authentication =
      typeof existingPolicy.authentication === 'string' &&
      VALID_AUTH_POLICIES.has(existingPolicy.authentication)
        ? existingPolicy.authentication
        : entry.policy.authentication;
    document.plugins[entryIndex] = {
      ...existingEntry,
      name: pluginName,
      source: entry.source,
      policy: {
        ...existingPolicy,
        installation,
        authentication,
      },
      category:
        typeof existingEntry.category === 'string' ? existingEntry.category : entry.category,
    } satisfies MarketplaceEntry;
  } else {
    document.plugins.push(entry);
  }

  return {
    document,
    marketplaceName: document.name,
    entryExisted: entryIndex >= 0,
    changed: !original || originalSnapshot !== JSON.stringify(document),
  };
}

function validateMarketplaceDocument(
  value: Record<string, unknown>,
  marketplacePath: string
): MarketplaceDocument {
  if (typeof value.name !== 'string' || !value.name.trim()) {
    throw new Error(`${marketplacePath} must contain a non-empty marketplace name`);
  }
  if (!Array.isArray(value.plugins)) {
    throw new Error(`${marketplacePath} field 'plugins' must be an array`);
  }
  if (value.interface !== undefined && !isObject(value.interface)) {
    throw new Error(`${marketplacePath} field 'interface' must be an object`);
  }
  return value as MarketplaceDocument;
}

function marketplaceEntry(
  pluginName: string,
  sourcePath: string,
  category: string
): MarketplaceEntry {
  return {
    name: pluginName,
    source: { source: 'local', path: sourcePath },
    policy: { installation: 'AVAILABLE', authentication: 'ON_INSTALL' },
    category,
  };
}

function planPluginSource(
  sourcePath: string,
  pluginRoot: string,
  pluginName: string,
  force: boolean
): SourcePlan {
  if (path.resolve(sourcePath) === pluginRoot) {
    throw new Error('marketplace source path resolves to the working plugin directory');
  }

  const sourceStats = lstatIfPresent(sourcePath);
  if (!sourceStats) return 'create';

  if (sourceStats.isSymbolicLink()) {
    try {
      if (realpathSync(sourcePath) === pluginRoot) return 'replace';
    } catch {
      // A broken link is a conflict and requires explicit replacement.
    }
    if (!force) {
      throw new Error(`${sourcePath} is a conflicting link; rerun with --force to replace it`);
    }
    return 'replace';
  }

  if (!sourceStats.isDirectory()) {
    if (!force) {
      throw new Error(`${sourcePath} is a conflicting file; rerun with --force to replace it`);
    }
    return 'replace';
  }

  const markerPath = path.join(sourcePath, SOURCE_MARKER);
  if (!existsSync(markerPath)) {
    if (!force) {
      throw new Error(
        `${sourcePath} is not a managed plugin source; rerun with --force to replace it`
      );
    }
    return 'replace';
  }

  let marker: Record<string, unknown>;
  try {
    marker = readJsonObject(markerPath, 'plugin source marker');
  } catch (error) {
    if (force) return 'replace';
    throw new Error(
      `${sourcePath} has an invalid source marker; rerun with --force to replace it: ${errorMessage(error)}`
    );
  }

  const belongsToPlugin =
    marker.schemaVersion === 1 &&
    marker.pluginName === pluginName &&
    typeof marker.pluginRoot === 'string' &&
    path.resolve(marker.pluginRoot) === pluginRoot;
  if (belongsToPlugin) return 'update';
  if (!force) {
    throw new Error(
      `${sourcePath} belongs to another plugin source; rerun with --force to replace it`
    );
  }
  return 'replace';
}

function lstatIfPresent(sourcePath: string): Stats | undefined {
  try {
    return lstatSync(sourcePath);
  } catch (error) {
    if (error instanceof Error && 'code' in error && error.code === 'ENOENT') return undefined;
    throw error;
  }
}

async function syncPluginSource(
  sourcePlan: SourcePlan,
  sourcePath: string,
  pluginRoot: string,
  pluginName: string
): Promise<void> {
  const nonce = `${process.pid}-${Date.now().toString(36)}`;
  const stagingPath = `${sourcePath}.staging-${nonce}`;
  const backupPath = `${sourcePath}.backup-${nonce}`;
  let backupCreated = false;

  mkdirSync(path.dirname(sourcePath), { recursive: true });
  rmSync(stagingPath, { recursive: true, force: true });
  rmSync(backupPath, { recursive: true, force: true });

  try {
    mkdirSync(stagingPath, { recursive: true });
    copyPluginFiles(pluginRoot, stagingPath);
    writeJsonAtomic(path.join(stagingPath, SOURCE_MARKER), {
      schemaVersion: 1,
      pluginName,
      pluginRoot,
      generatedAt: new Date().toISOString(),
    });

    await runCommand(
      process.execPath,
      ['install', '--production', '--frozen-lockfile', '--ignore-scripts', '--linker=hoisted'],
      stagingPath
    );
    removeDependencyBinDirectories(path.join(stagingPath, 'node_modules'));
    assertNoSymbolicLinks(stagingPath);

    if (sourcePlan === 'create' && lstatIfPresent(sourcePath)) {
      throw new Error(
        `${sourcePath} appeared while preparing the plugin source; refusing to replace it`
      );
    }
    if (lstatIfPresent(sourcePath)) {
      renameSync(sourcePath, backupPath);
      backupCreated = true;
    }

    try {
      renameSync(stagingPath, sourcePath);
    } catch (error) {
      if (backupCreated) {
        try {
          renameSync(backupPath, sourcePath);
          backupCreated = false;
        } catch (rollbackError) {
          throw new Error(
            `unable to activate ${sourcePath}; the previous source remains at ${backupPath}: ${errorMessage(error)}; rollback failed: ${errorMessage(rollbackError)}`
          );
        }
      }
      throw error;
    }

    if (backupCreated) {
      rmSync(backupPath, { recursive: true });
      backupCreated = false;
    }
    const action =
      sourcePlan === 'create' ? 'Created' : sourcePlan === 'update' ? 'Updated' : 'Replaced';
    writeLine(`${action} plugin source snapshot: ${sourcePath}`);
  } finally {
    rmSync(stagingPath, { recursive: true, force: true });
    if (!backupCreated) rmSync(backupPath, { recursive: true, force: true });
  }
}

function copyPluginFiles(pluginRoot: string, destinationRoot: string): void {
  const packageJson = readJsonObject(path.join(pluginRoot, 'package.json'), 'package manifest');
  if (
    !Array.isArray(packageJson.files) ||
    !packageJson.files.every(
      (entry): entry is string => typeof entry === 'string' && entry.length > 0
    )
  ) {
    throw new Error(`${path.join(pluginRoot, 'package.json')} must contain a string files array`);
  }

  const sourceEntries = new Set([
    'package.json',
    'bun.lock',
    'LICENSE',
    'README.md',
    ...packageJson.files,
  ]);
  for (const entry of sourceEntries) {
    const relativePath = normalizePackagePath(pluginRoot, entry);
    copySourceTree(path.join(pluginRoot, relativePath), path.join(destinationRoot, relativePath));
  }
}

function normalizePackagePath(pluginRoot: string, entry: string): string {
  const normalizedEntry = entry.replace(/^\.\/+/, '');
  if (!normalizedEntry || path.isAbsolute(normalizedEntry)) {
    throw new Error(`package files entry must be relative: ${entry}`);
  }
  const absolutePath = path.resolve(pluginRoot, normalizedEntry);
  const relativePath = path.relative(pluginRoot, absolutePath);
  if (
    relativePath === '..' ||
    relativePath.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relativePath)
  ) {
    throw new Error(`package files entry leaves the plugin root: ${entry}`);
  }
  return relativePath;
}

function copySourceTree(sourcePath: string, destinationPath: string): void {
  const sourceStats = lstatIfPresent(sourcePath);
  if (!sourceStats) throw new Error(`plugin source entry does not exist: ${sourcePath}`);
  if (sourceStats.isSymbolicLink()) {
    throw new Error(`plugin source entry may not be a symbolic link: ${sourcePath}`);
  }
  if (sourceStats.isDirectory()) {
    if (IGNORED_SOURCE_DIRECTORIES.has(path.basename(sourcePath))) return;
    mkdirSync(destinationPath, { recursive: true });
    for (const name of readdirSync(sourcePath).sort()) {
      copySourceTree(path.join(sourcePath, name), path.join(destinationPath, name));
    }
    return;
  }
  if (!sourceStats.isFile()) {
    throw new Error(`plugin source entry must be a file or directory: ${sourcePath}`);
  }
  mkdirSync(path.dirname(destinationPath), { recursive: true });
  copyFileSync(sourcePath, destinationPath);
  chmodSync(destinationPath, sourceStats.mode & 0o777);
}

function removeDependencyBinDirectories(directoryPath: string): void {
  const directoryStats = lstatIfPresent(directoryPath);
  if (!directoryStats || !directoryStats.isDirectory() || directoryStats.isSymbolicLink()) return;
  for (const name of readdirSync(directoryPath)) {
    const childPath = path.join(directoryPath, name);
    if (name === '.bin') {
      rmSync(childPath, { recursive: true, force: true });
      continue;
    }
    removeDependencyBinDirectories(childPath);
  }
}

function assertNoSymbolicLinks(sourcePath: string): void {
  const sourceStats = lstatSync(sourcePath);
  if (sourceStats.isSymbolicLink()) {
    throw new Error(`Codex local plugin sources may not contain symbolic links: ${sourcePath}`);
  }
  if (!sourceStats.isDirectory()) return;
  for (const name of readdirSync(sourcePath)) {
    assertNoSymbolicLinks(path.join(sourcePath, name));
  }
}

function withCachebuster(version: string, rawCachebuster: string): string {
  const cachebuster = rawCachebuster
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, '-')
    .replace(/-{2,}/g, '-')
    .replace(/^-|-$/g, '');
  if (!cachebuster) throw new Error('cachebuster must contain a letter or digit');
  return `${version.split('+', 1)[0]}+codex.${cachebuster}`;
}

function defaultCachebuster(): string {
  return new Date().toISOString().replace(/\D/g, '').slice(0, 14);
}

function assertBuildOutputs(pluginRoot: string): void {
  for (const output of ['index.js', 'codex-mcp.js', 'codex-hook.js']) {
    const outputPath = path.join(pluginRoot, 'dist', output);
    if (!existsSync(outputPath)) throw new Error(`build did not create ${outputPath}`);
  }
}

async function runCommand(command: string, args: string[], cwd: string): Promise<void> {
  writeLine(`> ${[command, ...args].map(shellDisplay).join(' ')}`);
  const child = Bun.spawn([command, ...args], {
    cwd,
    stdin: 'inherit',
    stdout: 'inherit',
    stderr: 'inherit',
  });
  const exitCode = await child.exited;
  if (exitCode !== 0) {
    throw new Error(`${path.basename(command)} exited with code ${exitCode}`);
  }
}

function shellDisplay(value: string): string {
  return /^[A-Za-z0-9_./:@+-]+$/.test(value) ? value : JSON.stringify(value);
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

function writeJsonAtomic(filePath: string, value: Record<string, unknown>): void {
  mkdirSync(path.dirname(filePath), { recursive: true });
  const temporaryPath = `${filePath}.tmp-${process.pid}-${Date.now().toString(36)}`;
  try {
    writeFileSync(temporaryPath, `${JSON.stringify(value, null, 2)}\n`, {
      encoding: 'utf8',
      mode: 0o600,
    });
    renameSync(temporaryPath, filePath);
  } finally {
    rmSync(temporaryPath, { force: true });
  }
}

function printPlan(plan: {
  pluginRoot: string;
  marketplacePath: string;
  marketplaceRoot: string;
  marketplaceName: string;
  sourcePath: string;
  sourcePlan: SourcePlan;
  shouldBuild: boolean;
  shouldCachebust: boolean;
  shouldRegisterMarketplace: boolean;
}): void {
  writeLine('Codex plugin install plan (dry run):');
  writeLine(`- plugin root: ${plan.pluginRoot}`);
  writeLine(`- marketplace: ${plan.marketplaceName} (${plan.marketplacePath})`);
  writeLine(`- marketplace root: ${plan.marketplaceRoot}`);
  writeLine(`- source snapshot: ${plan.sourcePath} (${plan.sourcePlan})`);
  writeLine(`- build host bundles: ${plan.shouldBuild ? 'yes' : 'no'}`);
  writeLine(`- update cachebuster: ${plan.shouldCachebust ? 'yes' : 'no'}`);
  writeLine(`- register marketplace with Codex: ${plan.shouldRegisterMarketplace ? 'yes' : 'no'}`);
  writeLine(`- install selector: ft-financial-canvas@${plan.marketplaceName}`);
}

function displayName(value: string): string {
  return value
    .split(/[-_]+/)
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(' ');
}

function expandHome(value: string): string {
  if (value === '~') return homedir();
  if (value.startsWith('~/') || value.startsWith('~\\'))
    return path.join(homedir(), value.slice(2));
  return value;
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

main().catch((error: unknown) => {
  process.stderr.write(`ft-financial-canvas install failed: ${errorMessage(error)}\n`);
  process.exitCode = 1;
});
