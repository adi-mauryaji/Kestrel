#!/usr/bin/env bash
# The always-on assistant (ADR-034): a systemd user service runs `kestrel serve` from login on, with
# rules + encoder loaded (~300 MB); Gemma loads only when a request needs it and unloads after 10
# idle minutes. Talk to it with `kestrel chat` or `kestrel say ...` (instant: nothing loads), or in
# the browser: `kestrel open`, or "Kestrel" in the applications menu (installed here too).
# `kestrel stop` stops it until the next open, chat or say (which start it again) or the next login.
#
#   scripts/install_service.sh ~/notes      install and start (or set KESTREL_NOTES)
#   scripts/install_service.sh --remove     stop and remove the service (notes and tasks are kept)
#
# The service runs its own copy of the binary ($DATA/kestrel/bin/kestrel, shared with the
# reminders), from this repository (models/, configs and prompt contracts are read from here); run
# the script again after an update.
set -euo pipefail
cd "$(dirname "$0")/.."

UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN_DIR="$DATA/kestrel/bin"
APP="$DATA/applications/kestrel.desktop"
LINK="$HOME/.local/bin/kestrel"
ICON="$DATA/icons/hicolor/scalable/apps/kestrel.svg"
# The Gemma worker's saved prompt prefix (ADR-036): valid only for the binary that wrote it.
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/kestrel"
CONFIG=experiments/configs/router_intents_v0.json

if [[ ${1:-} == --remove ]]; then
  systemctl --user disable --now kestrel.service 2>/dev/null || true
  rm -f "$UNIT_DIR/kestrel.service" "$APP" "$ICON" "$CACHE"/tier4-*.kv
  # The command on PATH goes only if it is still ours.
  if [[ -L $LINK && $(readlink "$LINK") == "$BIN_DIR/kestrel" ]]; then rm -f "$LINK"; fi
  systemctl --user daemon-reload
  echo "the always-on assistant is removed (notes and tasks are kept)"
  exit 0
fi

NOTES="${1:-${KESTREL_NOTES:-}}"
if [[ -z $NOTES ]]; then
  echo "usage: scripts/install_service.sh NOTES_DIR   (or set KESTREL_NOTES)" >&2
  exit 2
fi
NOTES="$(cd "$NOTES" 2>/dev/null && pwd)" || {
  echo "no notes folder at ${1:-$KESTREL_NOTES}: create it (mkdir -p) and put your notes in it" >&2
  exit 2
}

cargo build --release -p kestrel-cli --features llama
mkdir -p "$BIN_DIR" "$UNIT_DIR"
install -m 0755 target/release/kestrel "$BIN_DIR/kestrel"
rm -f "$CACHE"/tier4-*.kv

ENV_LINE=""
if [[ -n ${KESTREL_MEMORY:-} ]]; then
  ENV_LINE="Environment=\"KESTREL_MEMORY=$KESTREL_MEMORY\""
fi
cat > "$UNIT_DIR/kestrel.service" <<EOF
[Unit]
Description=Kestrel assistant (always on; the SLM loads on demand)

[Service]
WorkingDirectory=$PWD
$ENV_LINE
ExecStart="$BIN_DIR/kestrel" serve --notes "$NOTES" $CONFIG
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
EOF

# `kestrel` on PATH ($HOME/.local/bin), unless something else already has that name.
if [[ ! -e $LINK && ! -L $LINK ]]; then
  mkdir -p "$(dirname "$LINK")"
  ln -s "$BIN_DIR/kestrel" "$LINK"
fi

# The menu entry opens the page of the running service.
mkdir -p "$(dirname "$APP")" "$(dirname "$ICON")"
install -m 0644 apps/cli/web/kestrel.svg "$ICON"
cat > "$APP" <<EOF
[Desktop Entry]
Type=Application
Name=Kestrel
Comment=Your notes and tasks, on this laptop
Exec="$BIN_DIR/kestrel" open
Icon=kestrel
Terminal=false
Categories=Office;Utility;
EOF

systemctl --user daemon-reload
systemctl --user enable kestrel.service
# restart, not just start: after an update the running service must pick up the new copy
systemctl --user restart kestrel.service
echo "Kestrel is on (notes: $NOTES). Try: kestrel chat, or kestrel open for the page"
