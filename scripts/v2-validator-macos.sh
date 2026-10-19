#!/bin/bash
# ORVEXA v2 validator on this Mac: one-validator public test network that
# keeps running in the background (launchd) while the Mac is on, even with
# the app closed. See docs/V2_NETWORK_OPERATOR.md.
#
#   bash scripts/v2-validator-macos.sh create --to <ovxt1 address> --url https://v2.example.org [--name NAME]
#   bash scripts/v2-validator-macos.sh start | stop | status | logs
#   bash scripts/v2-validator-macos.sh set-url https://v2.example.org
#   bash scripts/v2-validator-macos.sh publish      # signs docs/networks/directory.json
#   bash scripts/v2-validator-macos.sh tunnel-help
#
# ORVEXA_V2_HOME  where keys and data live (default: ~/Library/Application Support/ORVEXA v2 validator)
# ORVEXA_V2_PORT  local port of the node (default 4260)
set -euo pipefail
cd "$(dirname "$0")/.."
repo="$PWD"
[[ "$(uname -s)" == Darwin ]] || { echo 'Run this on a Mac.' >&2; exit 1; }
home="${ORVEXA_V2_HOME:-$HOME/Library/Application Support/ORVEXA v2 validator}"
port="${ORVEXA_V2_PORT:-4260}"
label="org.orvexa.v2-validator"
plist="$HOME/Library/LaunchAgents/$label.plist"
bin="$home/bin/orvexa-v2-node"
network="$home/network.json"
key="$home/validator.key"
log="$home/node.log"

die() { echo "ORVEXA v2: $*" >&2; exit 1; }
need_network() { [[ -f "$network" && -f "$key" && -x "$bin" ]] || die "no network here yet; run: $0 create --to <ovxt1…> --url https://…"; }
valid_url() { [[ "$1" =~ ^https?://[^[:space:]/]+(:[0-9]+)?(/[^[:space:]]*[^/[:space:]])?$ ]]; }

cmd="${1:-}"; shift || true
case "$cmd" in
  create)
    to="" url="" name="ORVEXA v2 test"
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --to) to="$2"; shift 2 ;;
        --url) url="${2%/}"; shift 2 ;;
        --name) name="$2"; shift 2 ;;
        *) die "unknown option $1" ;;
      esac
    done
    [[ -n "$to" && -n "$url" ]] || die "usage: $0 create --to <ovxt1 address> --url https://… [--name NAME]"
    valid_url "$url" || die "--url must be http(s)://host without a trailing slash"
    [[ -e "$network" ]] && die "a network already exists in $home; it is never overwritten"
    mkdir -p "$home/bin" "$home/data"
    chmod 700 "$home"
    cargo build --release --locked -p orvexa-shielded --bin orvexa-v2-node
    cp target/release/orvexa-v2-node "$bin"
    public=$("$bin" keygen --out "$key")
    "$bin" genesis --to "$to" --validators "$public@$url" --nodes "$url" --name "$name" --out "$network"
    echo "Network created in $home"
    echo "Validator key: $key (back it up; never share it)"
    echo "Next: $0 start, then make $url reach 127.0.0.1:$port ($0 tunnel-help), then $0 publish"
    ;;
  start)
    need_network
    mkdir -p "$(dirname "$plist")"
    cat > "$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key>
  <array>
    <string>$bin</string><string>run</string>
    <string>--data-dir</string><string>$home/data</string>
    <string>--network</string><string>$network</string>
    <string>--validator-key</string><string>$key</string>
    <string>--listen</string><string>127.0.0.1:$port</string>
    <string>--block-every-ms</string><string>2000</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ProcessType</key><string>Background</string>
  <key>StandardOutPath</key><string>$log</string>
  <key>StandardErrorPath</key><string>$log</string>
</dict>
</plist>
EOF
    launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
    launchctl bootstrap "gui/$(id -u)" "$plist"
    for _ in $(seq 1 40); do curl -fsS "http://127.0.0.1:$port/v2/status" >/dev/null 2>&1 && break; sleep 0.5; done
    curl -fsS "http://127.0.0.1:$port/v2/status" || die "the node did not start; see $log"
    echo
    echo "Validator running at http://127.0.0.1:$port (starts again after login)."
    ;;
  stop)
    launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
    rm -f "$plist"
    echo "Validator stopped. Keys and chain stay in $home."
    ;;
  status)
    need_network
    curl -fsS "http://127.0.0.1:$port/v2/status" || die "not running (start it with: $0 start)"
    echo
    python3 -c 'import json,sys; p=json.load(open(sys.argv[1])); print("public address:", ", ".join(v["url"] for v in p["validators"]) or "none")' "$network"
    ;;
  logs)
    tail -n 50 -f "$log"
    ;;
  set-url)
    need_network
    url="${1%/}"
    valid_url "$url" || die "usage: $0 set-url https://host"
    # The address is not part of the chain ID: the network stays the same.
    python3 - "$network" "$url" <<'EOF'
import json, sys
path, url = sys.argv[1], sys.argv[2]
p = json.load(open(path))
for v in p["validators"]:
    v["url"] = url
p["nodes"] = [url]
open(path, "w").write(json.dumps(p, ensure_ascii=False, indent=2) + "\n")
EOF
    "$bin" directory --key "$key" --profile "$network" --out "$home/.check.json" >/dev/null && rm -f "$home/.check.json" "$home/.check.json.sig"
    echo "Public address set to $url. Restart ($0 start) and publish again ($0 publish)."
    ;;
  publish)
    need_network
    release_key="${ORVEXA_RELEASE_KEY:-$HOME/.orvexa-release/update.key}"
    out="${ORVEXA_V2_DIRECTORY_OUT:-$repo/docs/networks/directory.json}"
    [[ -f "$release_key" ]] || die "release key $release_key not found"
    mkdir -p "$(dirname "$out")"
    "$bin" directory --key "$release_key" --profile "$network" --out "$out"
    echo "Commit $(dirname "$out") to main: GitHub Pages publishes it, and the app's"
    echo "\"Connect to the ORVEXA v2 network\" button finds this network."
    ;;
  tunnel-help)
    cat <<EOF
The network needs one public HTTPS address that reaches http://127.0.0.1:$port
on this Mac. With Cloudflare (free account, a domain added to Cloudflare):

  cloudflared tunnel login
  cloudflared tunnel create orvexa-v2
  cloudflared tunnel route dns orvexa-v2 v2.<your-domain>
  cloudflared tunnel run --url http://127.0.0.1:$port orvexa-v2

(sudo cloudflared service install keeps it running.) Then:

  $0 set-url https://v2.<your-domain>
  $0 start
  $0 publish

A quick tunnel (cloudflared tunnel --url http://127.0.0.1:$port) works for a
test, but its address changes every time, so others lose the network.
EOF
    ;;
  *)
    sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac
