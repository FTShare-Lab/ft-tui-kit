#!/usr/bin/env bun

import { existsSync } from 'node:fs';
import path from 'node:path';

const binaryName = process.platform === 'win32' ? 'dag.exe' : 'dag';
const platformDirectory = `${process.platform}-${process.arch}`;
const binaryPath = path.join(import.meta.dir, 'bin', platformDirectory, binaryName);

if (!existsSync(binaryPath)) {
  throw new Error(`DAG renderer does not include a binary for ${platformDirectory}: ${binaryPath}`);
}

const subprocess = Bun.spawn([binaryPath, ...process.argv.slice(2)], {
  stdin: 'inherit',
  stdout: 'inherit',
  stderr: 'inherit',
});

process.exit(await subprocess.exited);
