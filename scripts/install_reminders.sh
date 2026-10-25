#!/usr/bin/env bash
# Desktop reminders (ADR-034): a systemd user timer runs `kestrel remind --notify` every morning at
# 08:00 and two minutes after login; a morning missed while the laptop was off runs at the next
# login (Persistent). The notification lists what is due today and what is overdue, from the memory
# file; nothing pops up when nothing is due. Nothing leaves the machine.
#
#   scripts/install_reminders.sh            build, install a private copy of the binary, start the timer
#   scripts/install_reminders.sh --remove   stop and remove the timer (the memory file is kept)
#
# The timer runs its own copy of the binary ($DATA/kestrel/bin/kestrel), so switching branches or
# rebuilding target/ never breaks it; run this script again after an update to refresh the copy.
set -euo pipefail
cd "$(dirname "$0")/.."

UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN_DIR="$DATA/kestrel/bin"

if [[ ${1:-} == --remove ]]; then
  systemctl --user disable --now kestrel-remind.timer 2>/dev/null || true
  rm -f "$UNIT_DIR/kestrel-remind.service" "$UNIT_DIR/kestrel-remind.timer" "$BIN_DIR/kestrel"
  systemctl --user daemon-reload
  echo "reminders removed (tasks stay in the memory file)"
  exit 0
fi

cargo build --release -p kestrel-cli --features llama
mkdir -p "$BIN_DIR" "$UNIT_DIR"
install -m 0755 target/release/kestrel "$BIN_DIR/kestrel"

ENV_LINE=""
if [[ -n ${KESTREL_MEMORY:-} ]]; then
  ENV_LINE="Environment=\"KESTREL_MEMORY=$KESTREL_MEMORY\""
fi
cat > "$UNIT_DIR/kestrel-remind.service" <<EOF
[Unit]
Description=Kestrel: tasks due today and overdue

[Service]
Type=oneshot
$ENV_LINE
ExecStart="$BIN_DIR/kestrel" remind --notify
EOF
cat > "$UNIT_DIR/kestrel-remind.timer" <<EOF
[Unit]
Description=Kestrel reminders every morning and after login

[Timer]
OnCalendar=*-*-* 08:00:00
OnStartupSec=2min
Persistent=true

[Install]
WantedBy=timers.target
EOF

systemctl --user daemon-reload
systemctl --user enable --now kestrel-remind.timer
echo "reminders on: every day at 08:00 and 2 minutes after login"
systemctl --user list-timers kestrel-remind.timer --no-pager | head -3
