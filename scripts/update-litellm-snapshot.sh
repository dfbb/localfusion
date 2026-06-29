#!/usr/bin/env bash
# Refresh the bundled litellm price snapshot. Run manually to update the build-time default.
# The running server also refreshes this data daily into the DB (see src/price_refresh.rs).
set -euo pipefail
URL="https://raw.githubusercontent.com/BerriAI/litellm/litellm_internal_staging/model_prices_and_context_window.json"
DEST="$(dirname "$0")/../assets/litellm_model_prices.json"
mkdir -p "$(dirname "$DEST")"
curl -fsSL --max-time 60 "$URL" -o "$DEST"
echo "wrote $DEST ($(wc -c < "$DEST") bytes)"
