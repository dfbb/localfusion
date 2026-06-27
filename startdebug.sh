#!/usr/bin/env bash
# startdebug.sh — build debug binary and run LocalFusion with a scratch database
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
DB_DIR="$REPO_ROOT/target/tmp"
DB_PATH="$DB_DIR/localfusion.db"

cd "$REPO_ROOT"

echo "==> Building debug binary..."
cargo build

mkdir -p "$DB_DIR"

echo "==> Starting LocalFusion (debug) — db: $DB_PATH"
exec "$REPO_ROOT/target/debug/localfusion" --db "$DB_PATH" --debug
