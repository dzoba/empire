#!/usr/bin/env bash
# Serve web/ on http://localhost:${PORT:-8080}
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 scripts/serve.py "${PORT:-8080}"
