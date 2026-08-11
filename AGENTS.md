# AGENTS.md

## Project Context

- **Name**: ft financial canvas
- **Origin**: evolved from the Feitu July Hackathon prototype
- **Type**: public-source, local-development plugin with independent Canvas renderer processes
- **Target**: Bun and ES modules; Rust 1.88+ for native renderers
- **Focus**: terminal-native financial visualization and LLM analysis workflows
- **Boundary**: `src/` owns the plugin/controller; `canvases/` owns renderer manifests,
  source, launchers, and packaged binaries

Canvas v2 is the current IPC contract. Preserve the `canvas_*` tool names and protocol fields unless
a migration is part of the requested change.

## Setup, Build, and Checks

- Install: `bun install`
- Build all changed inputs: `bun run build` or `./ftopencode-build`
- Force all builds: `./ftopencode-build --force`
- Build plugin only: `bun run build:plugin`
- Type-check: `bun run typecheck`
- Lint: `bun run lint`
- Fix lint: `bun run lint:fix`
- Check formatting: `bun run format:check`
- Format TypeScript/TSX: `bun run format`
- Run all TypeScript checks: `bun run check`
- Native renderer tests: `cargo test -p <package-name>`
- Build all native renderers once: `cargo build --workspace --release --locked`

There is currently no root Vitest suite. Do not report `bun test` as project validation unless tests
are added.

## Code Style

### Imports and modules

- Use ES module `import`/`export` syntax.
- Group external imports before internal imports.
- Use explicit `.ts` extensions for internal imports when the existing module already does so.

### Formatting and naming

- Follow `.prettierrc`: single quotes, semicolons, two spaces, 100-column width.
- Avoid deep nesting; prefer early returns.
- Keep TypeScript strict and avoid `any` where a concrete type is available.
- Classes and exported React components use PascalCase; methods and values use camelCase.
- Renderer names and scenarios must match their `canvases/<name>/config.json` manifest.

### Errors and logging

- Narrow unknown errors before reading properties.
- Add actionable context to errors.
- Do not add routine `console` logging to plugin code.

## Architecture Rules

- The host discovers renderers from `canvases/*/config.json`; support folders such as `_sdk` are not
  renderers.
- Renderer-specific schemas and UI logic stay under their renderer directory.
- Shared renderer-side Rust protocol/runtime code stays in the `ft-canvas-runtime` crate under
  `canvases/_sdk-rust/`; it must not depend on plugin code or renderer-specific business logic.
- The plugin owns OpenCode session binding, runtime files, sockets, and tmux placement.
- Renderers communicate with the host only through the Canvas v2 protocol.
- Keep generated data in `.memory/`, `dist/`, or renderer `target/` directories; these are ignored.
