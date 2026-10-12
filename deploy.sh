#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

ROUTER="${ROUTER:-root@192.168.1.1}"
BIN="target/mipsel-unknown-linux-musl/release/tgbot"
INIT="openwrt/tgbot.init"

[ -x "$BIN" ] || { echo "error: $BIN missing - run ./build.sh first" >&2; exit 1; }

echo ">> copying binary + init script to $ROUTER"
scp -q "$BIN" "$ROUTER:/tmp/tgbot"
scp -q "$INIT" "$ROUTER:/etc/init.d/tgbot"

echo ">> installing and restarting service"
ssh "$ROUTER" '/bin/sh -s' <<'EOF'
cp /tmp/tgbot /usr/bin/tgbot && rm -f /tmp/tgbot
chmod +x /usr/bin/tgbot /etc/init.d/tgbot
if [ ! -f /etc/tgbot.conf ]; then
	echo "!! /etc/tgbot.conf missing - create it from openwrt/tgbot.conf.example"
else
	/etc/init.d/tgbot enable
	/etc/init.d/tgbot restart
	sleep 1
	if /etc/init.d/tgbot status >/dev/null 2>&1; then
		echo "service running"
	else
		echo "service not running - check: logread | grep tgbot"
	fi
fi
EOF

echo ">> deployed to $ROUTER"
