#!/usr/bin/env bash
# macOS 客户端 LaunchAgent 常驻守护进程与 Finder Quick Action 右键安装脚本
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_SUPPORT_DIR="$HOME/Library/Application Support/multinteractor"
LAUNCH_AGENTS_DIR="$HOME/Library/LaunchAgents"
PLIST_NAME="com.multinteractor.daemon.plist"

mkdir -p "$APP_SUPPORT_DIR"
mkdir -p "$LAUNCH_AGENTS_DIR"
cp "$SCRIPT_DIR/$PLIST_NAME" "$LAUNCH_AGENTS_DIR/$PLIST_NAME"

echo "Installed LaunchAgent to $LAUNCH_AGENTS_DIR/$PLIST_NAME"
echo "App Support directory ready at $APP_SUPPORT_DIR"
