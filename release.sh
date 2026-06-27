#!/usr/bin/env bash
# release.sh — bump version, cross-compile for Windows/Linux/macOS, publish GitHub release
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$REPO_ROOT"

# ---------------------------------------------------------------------------
# 1. Helpers
# ---------------------------------------------------------------------------

die() { echo "ERROR: $*" >&2; exit 1; }

require_cmd() {
    command -v "$1" &>/dev/null || die "'$1' is required but not found. Install it and retry."
}

require_cmd git
require_cmd gh
require_cmd cargo
require_cmd zip

# ---------------------------------------------------------------------------
# 2. Read current version from Cargo.toml
# ---------------------------------------------------------------------------

CURRENT_VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')"
echo "Current version: $CURRENT_VERSION"

# ---------------------------------------------------------------------------
# 3. Show git log since last release tag
# ---------------------------------------------------------------------------

LAST_TAG="$(git tag --sort=-version:refname | grep '^release-' | head -1 || true)"

if [[ -z "$LAST_TAG" ]]; then
    echo ""
    echo "==> No previous release tag found. Showing full commit history:"
    git log --oneline
else
    echo ""
    echo "==> Commits since $LAST_TAG:"
    git log --oneline "${LAST_TAG}..HEAD"
fi

# ---------------------------------------------------------------------------
# 4. Ask user to confirm
# ---------------------------------------------------------------------------

echo ""
read -r -p "Proceed with release? [y/N] " CONFIRM
[[ "$CONFIRM" =~ ^[Yy]$ ]] || { echo "Release cancelled."; exit 0; }

# ---------------------------------------------------------------------------
# 5. Bump version: patch +1, carry at 9 (0.1.9 -> 0.2.0, 0.9.9 -> 1.0.0)
# ---------------------------------------------------------------------------

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"

PATCH=$((PATCH + 1))
if [[ $PATCH -gt 9 ]]; then
    PATCH=0
    MINOR=$((MINOR + 1))
fi
if [[ $MINOR -gt 9 ]]; then
    MINOR=0
    MAJOR=$((MAJOR + 1))
fi

NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
TAG="release-${NEW_VERSION}"

echo ""
echo "==> Bumping version: $CURRENT_VERSION -> $NEW_VERSION (tag: $TAG)"

# Update Cargo.toml
sed -i.bak "s/^version = \"${CURRENT_VERSION}\"/version = \"${NEW_VERSION}\"/" Cargo.toml
rm -f Cargo.toml.bak

# Update Cargo.lock (regenerate)
cargo generate-lockfile 2>/dev/null || true

# ---------------------------------------------------------------------------
# 6. Build frontend
# ---------------------------------------------------------------------------

echo ""
echo "==> Building frontend..."
(cd web && pnpm install --frozen-lockfile && pnpm build)

# ---------------------------------------------------------------------------
# 7. Cross-compile for all three platforms
# ---------------------------------------------------------------------------

DIST_DIR="$REPO_ROOT/target/dist/$NEW_VERSION"
mkdir -p "$DIST_DIR"

build_target() {
    local TARGET="$1"
    local LABEL="$2"
    local BIN_NAME="$3"    # binary filename on that platform

    echo ""
    echo "==> Building $LABEL ($TARGET)..."

    if command -v cross &>/dev/null; then
        cross build --release --target "$TARGET"
    else
        # Fall back to cargo — reqwest uses rustls-tls so no OpenSSL needed
        echo "    (cross not installed — using cargo)"
        cargo build --release --target "$TARGET"
    fi

    local BIN_PATH="$REPO_ROOT/target/${TARGET}/release/${BIN_NAME}"
    [[ -f "$BIN_PATH" ]] || die "Binary not found at $BIN_PATH"

    local ZIP_NAME="localfusion-${NEW_VERSION}-${LABEL}.zip"
    local ZIP_PATH="$DIST_DIR/$ZIP_NAME"

    echo "    Packaging $ZIP_NAME..."
    (cd "$(dirname "$BIN_PATH")" && zip -j "$ZIP_PATH" "$BIN_NAME")

    echo "    Created: $ZIP_PATH"
}

# macOS Apple Silicon
build_target "aarch64-apple-darwin"  "macos-arm64"  "localfusion"

# Linux x86_64 (requires cross or a Linux toolchain)
rustup target add x86_64-unknown-linux-musl 2>/dev/null || true
build_target "x86_64-unknown-linux-musl" "linux-x86_64" "localfusion"

# Windows x86_64 (requires cross or mingw toolchain)
rustup target add x86_64-pc-windows-gnu 2>/dev/null || true
build_target "x86_64-pc-windows-gnu" "windows-x86_64" "localfusion.exe"

# ---------------------------------------------------------------------------
# 8. Commit version bump, tag, push
# ---------------------------------------------------------------------------

echo ""
echo "==> Committing version bump and pushing..."
git add Cargo.toml Cargo.lock
git commit -m "chore: release v${NEW_VERSION}"
git tag "$TAG"
git push origin main
git push origin "$TAG"

# ---------------------------------------------------------------------------
# 9. Create GitHub release and upload assets
# ---------------------------------------------------------------------------

echo ""
echo "==> Creating GitHub release $TAG..."

# Build release notes from git log
if [[ -z "$LAST_TAG" ]]; then
    NOTES="$(git log --oneline HEAD~20..HEAD 2>/dev/null || git log --oneline)"
else
    NOTES="$(git log --oneline "${LAST_TAG}..HEAD")"
fi

gh release create "$TAG" \
    --title "LocalFusion v${NEW_VERSION}" \
    --notes "## What's Changed

${NOTES}" \
    "$DIST_DIR"/localfusion-"${NEW_VERSION}"-*.zip

echo ""
echo "==> Release $TAG published successfully."
echo "    https://github.com/$(gh repo view --json nameWithOwner -q .nameWithOwner)/releases/tag/${TAG}"
