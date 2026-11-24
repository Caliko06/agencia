#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DST="${HOME}/.local/bin"
BINARY="${REPO_DIR}/target/release/agencia"

echo "==> Building Agencia (release)..."
(cd "${REPO_DIR}" && cargo build --release --workspace)

echo "==> Linking multicall CLI tools in ${REPO_DIR}/bin and ${BIN_DST}..."
mkdir -p "${BIN_DST}" "${REPO_DIR}/bin"
for tool in agencia-up agshot agy-send bec cdx cla real-fin sesiones-bus slice; do
  ln -sfn "../target/release/agencia" "${REPO_DIR}/bin/${tool}"
done
ln -sfn "${BINARY}" "${BIN_DST}/agencia"
for tool in bec cdx cla slice real-fin agshot agy-send sesiones-bus; do
  ln -sfn "${REPO_DIR}/bin/${tool}" "${BIN_DST}/${tool}"
done

if [ "${1:-}" = "--launchd" ]; then
  PLIST="${HOME}/Library/LaunchAgents/com.agencia.server.plist"
  mkdir -p "$(dirname "${PLIST}")" "${REPO_DIR}/cache"
  cat > "${PLIST}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.agencia.server</string>
  <key>ProgramArguments</key><array><string>${BINARY}</string><string>serve</string></array>
  <key>WorkingDirectory</key><string>${REPO_DIR}</string>
  <key>EnvironmentVariables</key><dict><key>PATH</key><string>${HOME}/.local/bin:${HOME}/.opencode/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin</string></dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>StandardOutPath</key><string>${REPO_DIR}/cache/server.log</string>
  <key>StandardErrorPath</key><string>${REPO_DIR}/cache/server.log</string>
</dict></plist>
EOF
  UID_NOW="$(id -u)"
  launchctl bootout "gui/${UID_NOW}/com.agencia.server" 2>/dev/null || true
  launchctl bootstrap "gui/${UID_NOW}" "${PLIST}"
  echo "==> LaunchAgent com.agencia.server loaded on http://127.0.0.1:8800"
fi

if ! echo ":${PATH}:" | grep -q ":${BIN_DST}:"; then
  echo "==> Add ${BIN_DST} to your PATH to run 'agencia' from any directory:"
  echo "    echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
fi
echo "==> Done! Run 'agencia' to open http://127.0.0.1:8800."
