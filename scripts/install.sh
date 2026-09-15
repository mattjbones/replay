#!/usr/bin/env bash
# Recap installer for macOS (Apple Silicon).
#
# Downloads the latest release DMG with curl (which, unlike a browser, does not
# set the com.apple.quarantine attribute), installs Recap.app to /Applications,
# and optionally sets up the background sync daemon and the Claude Code MCP
# server. Because nothing is quarantined, Gatekeeper never prompts.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mattjbones/replay/main/scripts/install.sh | bash
#
# Options (env vars):
#   RECAP_VERSION=v0.3.1     install a specific tag instead of the latest
#   RECAP_DAEMON=0           skip LaunchAgent install (default: install)
#   RECAP_MCP=0              skip Claude Code MCP registration (default: register if `claude` is on PATH)
#   RECAP_APP_DIR=~/Applications   install somewhere other than /Applications
set -euo pipefail

REPO="mattjbones/replay"
ASSET="Recap_aarch64.dmg"
APP_DIR="${RECAP_APP_DIR:-/Applications}"
APP_PATH="$APP_DIR/Recap.app"
DAEMON="$APP_PATH/Contents/MacOS/recap-daemon"

log()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || fail "Recap is macOS-only."
[[ "$(uname -m)" == "arm64" ]]  || fail "Only an Apple Silicon build is published (this Mac is $(uname -m))."

if [[ -n "${RECAP_VERSION:-}" ]]; then
  TAG="$RECAP_VERSION"
else
  TAG=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
  [[ -n "$TAG" ]] || fail "could not determine latest release tag"
fi
URL="https://github.com/$REPO/releases/download/$TAG/$ASSET"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"; [[ -n "${MNT:-}" ]] && hdiutil detach "$MNT" -quiet 2>/dev/null || true' EXIT

log "Downloading Recap $TAG"
curl -fL --progress-bar -o "$TMP/$ASSET" "$URL"

log "Mounting image"
MNT=$(hdiutil attach -nobrowse -readonly "$TMP/$ASSET" | awk -F'\t' '/\/Volumes\//{print $NF}')
[[ -d "$MNT/Recap.app" ]] || fail "Recap.app not found in DMG"

if pgrep -qf "$APP_PATH/Contents/MacOS/recap-app"; then
  log "Quitting running Recap"
  osascript -e 'quit app "Recap"' >/dev/null 2>&1 || true
  sleep 2
fi

log "Installing to $APP_PATH"
mkdir -p "$APP_DIR"
rm -rf "$APP_PATH"
ditto "$MNT/Recap.app" "$APP_PATH"
hdiutil detach "$MNT" -quiet; MNT=""
# Belt and braces: nothing above should have quarantined it, but if the
# script itself was downloaded by a browser, inherited flags are possible.
xattr -dr com.apple.quarantine "$APP_PATH" 2>/dev/null || true

VERSION=$(defaults read "$APP_PATH/Contents/Info.plist" CFBundleShortVersionString 2>/dev/null || echo "?")
log "Installed Recap $VERSION"

if [[ "${RECAP_DAEMON:-1}" == "1" && -x "$DAEMON" ]]; then
  log "Installing background sync daemon (LaunchAgent)"
  "$DAEMON" install
fi

if [[ "${RECAP_MCP:-1}" == "1" && -x "$DAEMON" ]] && command -v claude >/dev/null 2>&1; then
  log "Registering MCP server with Claude Code (user scope)"
  claude mcp remove --scope user recap >/dev/null 2>&1 || true
  claude mcp add --scope user recap -- "$DAEMON" mcp
fi

log "Launching Recap"
open -a "$APP_PATH"

cat <<MSG

Done. Recap $VERSION is installed.

  App:     $APP_PATH
  Daemon:  $DAEMON  (status | install | uninstall | sync | mcp)
  MCP:     add to any client as:  "$DAEMON" mcp

To update later, run this script again.
MSG
