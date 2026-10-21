#!/bin/bash
# Browser-relay for Telegram Web and FB phone recovery
# Uses Chrome DevTools Protocol via Chrome/Chromium

set -euo pipefail

CHROME_PORT=9222
CHROME_DIR="$HOME/.chrome-relay"
CHROME_USER_DATA="$CHROME_DIR/user-data"

mkdir -p "$CHROME_USER_DATA"

# Start Chrome with remote debugging if not running
start_chrome() {
    if ! lsof -i :$CHROME_PORT >/dev/null 2>&1; then
        google-chrome-stable \
            --remote-debugging-port=$CHROME_PORT \
            --user-data-dir="$CHROME_USER_DATA" \
            --no-first-run \
            --no-default-browser-check \
            --disable-background-timer-throttling \
            --disable-backgrounding-occluded-windows \
            --disable-renderer-backgrounding \
            >/dev/null 2>&1 &
        sleep 2
    fi
}

# Get WebSocket debugger URL
get_ws_url() {
    curl -s "http://localhost:$CHROME_PORT/json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for tab in data:
    if tab.get('type') == 'page':
        print(tab['webSocketDebuggerUrl'])
        break
"
}

# Send CDP command
cdp_cmd() {
    local ws_url="$1"
    local method="$2"
    local params="$3"
    python3 << 'PYEOF'
import asyncio, websockets, json, sys, os
ws_url = os.environ.get('CDP_WS_URL')
method = os.environ.get('CDP_METHOD')
params = os.environ.get('CDP_PARAMS', '{}')
async def send():
    async with websockets.connect(ws_url) as ws:
        msg = {"id": 1, "method": method, "params": json.loads(params)}
        await ws.send(json.dumps(msg))
        response = await ws.recv()
        print(response)
asyncio.run(send())
PYEOF
}

# Navigate to URL
navigate() {
    local ws_url="$1"
    local url="$2"
    export CDP_WS_URL="$ws_url"
    export CDP_METHOD="Page.navigate"
    export CDP_PARAMS="{\"url\": \"$url\"}"
    cdp_cmd "$ws_url" "Page.navigate" "{\"url\": \"$url\"}"
}

case "${1:-}" in
    start)
        start_chrome
        echo "Chrome started on port $CHROME_PORT"
        ;;
    ws)
        get_ws_url
        ;;
    telegram)
        start_chrome
        WS_URL=$(get_ws_url)
        navigate "$WS_URL" "https://web.telegram.org/a/#8954499776"
        echo "Opened Telegram Web"
        ;;
    telegram-k)
        start_chrome
        WS_URL=$(get_ws_url)
        navigate "$WS_URL" "https://web.telegram.org/k/#@zolly_manager_bot"
        echo "Opened Telegram Web K - zolly_manager_bot"
        ;;
    fb-phone)
        start_chrome
        WS_URL=$(get_ws_url)
        navigate "$WS_URL" "$2"
        echo "Opened FB page for phone recovery: $2"
        ;;
    navigate)
        start_chrome
        WS_URL=$(get_ws_url)
        navigate "$WS_URL" "$2"
        echo "Navigated to: $2"
        ;;
    *)
        echo "Usage: $0 {start|ws|telegram|telegram-k|fb-phone <url>|navigate <url>}"
        exit 1
        ;;
esac