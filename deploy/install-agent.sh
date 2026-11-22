#!/usr/bin/env bash
# Install TORTA Dice agent on Linux (systemd user unit)
set -euo pipefail

SERVER_URL="${1:?usage: install-agent.sh <server_url> <agent_token>}"
AGENT_TOKEN="${2:?}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/share/torta-dice}"
BIN_SRC="$(cd "$(dirname "$0")/.." && pwd)/target/release/dice-agent"

mkdir -p "$INSTALL_DIR" "$HOME/.config/dice" "$HOME/.config/systemd/user"
if [[ ! -x "$BIN_SRC" ]]; then
  echo "Build first: cargo build --release -p dice-agent" >&2
  exit 1
fi
cp -f "$BIN_SRC" "$INSTALL_DIR/dice-agent"
chmod +x "$INSTALL_DIR/dice-agent"

cat > "$HOME/.config/dice/dice-agent.toml" <<EOF
server_url = "$SERVER_URL"
agent_token = "$AGENT_TOKEN"
enabled = true
poll_interval_secs = 30
extra_paths = []
EOF

cat > "$HOME/.config/systemd/user/dice-agent.service" <<EOF
[Unit]
Description=TORTA Dice backup agent
After=network-online.target

[Service]
ExecStart=$INSTALL_DIR/dice-agent -c $HOME/.config/dice/dice-agent.toml run
Restart=on-failure
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
systemctl --user enable --now dice-agent.service
echo "Installed user service dice-agent"
echo "Tray mode (desktop): $INSTALL_DIR/dice-agent -c $HOME/.config/dice/dice-agent.toml tray"
