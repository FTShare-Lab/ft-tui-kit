# Distribution

The concrete release-archive and installer design is documented in [DELIVERY.md](./DELIVERY.md).

`ft financial canvas` is currently a public-source, local-development project. It is not yet
published to npm, and pushes to the repository do not trigger a package release.

## Create a Local Build

```bash
bun install --frozen-lockfile
bun run check
./ftopencode-build --force
```

The build creates `dist/index.js` and refreshes native renderer binaries for the current platform
when their source has changed. Test the result from a separate OpenCode workspace using a relative
path to either `src/index.ts` (development) or the project root (built-package simulation).

Before sharing a build, verify:

- OpenCode starts through `ftopencode` inside tmux;
- `canvas_renderers` lists every expected renderer;
- at least the `candlesticks`, `chart`, and `dag` renderers can start and close;
- packaged native binaries match the recipient platform and architecture;
- no credentials, private market data, runtime files, or Cargo `target/` directories are included.

## Before Any Public Release

A public release requires an explicit maintainer decision. At that point, choose the package scope
and repository URL, review third-party licenses and packaged binaries, remove `private: true`, and
add publishing automation tied to the real package owner. Do not restore package-owner metadata
from the original template; it does not identify this project.
