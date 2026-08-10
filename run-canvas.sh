#!/bin/bash
# Compatibility wrapper for the ft financial canvas renderer launcher.
cd "$(dirname "$0")"
exec bun run canvases/launcher.ts "$@"
