import { existsSync } from 'node:fs';
import path from 'node:path';

export function resolvePluginRoot(): string {
  const candidates = [
    process.env.FT_CANVAS_PLUGIN_ROOT,
    path.resolve(import.meta.dir, '../../..'),
    path.resolve(import.meta.dir, '..'),
    process.cwd(),
  ];

  for (const candidate of candidates) {
    if (!candidate) continue;
    if (
      existsSync(path.join(candidate, 'canvases')) &&
      existsSync(path.join(candidate, 'canvases', 'launcher.ts'))
    ) {
      return candidate;
    }
  }

  throw new Error(
    'Unable to locate the Financial Canvas plugin root. Set FT_CANVAS_PLUGIN_ROOT to the installed plugin directory.'
  );
}
