#!/usr/bin/env bash
# Assemble "Causewaybay Panda.app": a double-clickable shop for a Mac.
#
#   tools/mac_app.sh                # dist/Causewaybay Panda.app
#   open "dist/Causewaybay Panda.app"
#
# The bundle carries the release binary and the static site. Double-clicking
# starts the shop for this Mac's user, waits for it to answer, opens it in the
# default browser, and shows the address guests should type on their phones
# together with the owner pin. First run mints a random pin and keeps it in
# ~/.causewaybaypanda/owner-pin; the shop's data lives beside it.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
name="Causewaybay Panda"
app="$here/dist/$name.app"
bin="${PANDA_RELEASE_BIN:-$here/target/release/panda}"

if [ ! -x "$bin" ]; then
  echo "no release binary at $bin — run: cargo build --release -p causewaybay-panda-server" >&2
  exit 1
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/static"
cp "$bin" "$app/Contents/MacOS/panda"
cp -R "$here/static/." "$app/Contents/Resources/static/"

# An .icns from the shop's own icon, if the tools are here (they are on any Mac).
if command -v sips >/dev/null && command -v iconutil >/dev/null; then
  iconset="$(mktemp -d)/panda.iconset"
  mkdir -p "$iconset"
  for px in 16 32 64 128 256 512; do
    sips -z "$px" "$px" "$here/static/assets/icon.png" --out "$iconset/icon_${px}x${px}.png" >/dev/null 2>&1 || true
    sips -z "$((px * 2))" "$((px * 2))" "$here/static/assets/icon.png" --out "$iconset/icon_${px}x${px}@2x.png" >/dev/null 2>&1 || true
  done
  iconutil -c icns "$iconset" -o "$app/Contents/Resources/panda.icns" 2>/dev/null || true
fi

cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Causewaybay Panda</string>
  <key>CFBundleDisplayName</key><string>Causewaybay Panda</string>
  <key>CFBundleIdentifier</key><string>com.causewaybay.panda</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>0.1</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>launch</string>
  <key>CFBundleIconFile</key><string>panda</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHumanReadableCopyright</key><string>Causewaybay Coffee</string>
</dict>
</plist>
PLIST

# The launcher is what a double-click runs.
cat > "$app/Contents/MacOS/launch" <<'LAUNCH'
#!/bin/bash
# Start the shop and put its address in front of the owner.
set -u
here="$(cd "$(dirname "$0")/.." && pwd)"
export PANDA_ROOT="$here/Resources"
export PANDA_HOME="${PANDA_HOME:-$HOME/.causewaybaypanda}"
export PANDA_PORT="${PANDA_PORT:-8787}"
mkdir -p "$PANDA_HOME"

# A pin is minted once and kept; the demo default is not for a real shop.
pin_file="$PANDA_HOME/owner-pin"
if [ -z "${PANDA_OWNER_PIN:-}" ]; then
  if [ ! -s "$pin_file" ]; then
    printf '%04d' "$(( (RANDOM * 32768 + RANDOM) % 10000 ))" > "$pin_file"
    chmod 600 "$pin_file"
  fi
  export PANDA_OWNER_PIN="$(cat "$pin_file")"
fi

# Any keys the owner keeps in a plain env file beside the data.
# e.g.  GROK_API_KEY=...   PANDA_MODE=live   PANDA_TREASURY=0x...
if [ -f "$PANDA_HOME/env" ]; then
  set -a; . "$PANDA_HOME/env"; set +a
fi

log="$PANDA_HOME/panda.log"
health="http://127.0.0.1:$PANDA_PORT/health"

# Already open? Just bring the page up again.
if curl -sf "$health" >/dev/null 2>&1; then
  open "http://127.0.0.1:$PANDA_PORT"
  exit 0
fi

"$here/MacOS/panda" >> "$log" 2>&1 &
pid=$!
echo "$pid" > "$PANDA_HOME/panda.pid"

for _ in $(seq 1 100); do
  curl -sf "$health" >/dev/null 2>&1 && break
  sleep 0.1
done
if ! curl -sf "$health" >/dev/null 2>&1; then
  osascript -e 'display alert "Causewaybay Panda could not open" message "See '"$log"'" as critical' >/dev/null 2>&1 || true
  exit 1
fi

# The address guests type in. Prefer the wifi interface, fall back to any.
lan="$(ipconfig getifaddr en0 2>/dev/null || ipconfig getifaddr en1 2>/dev/null || echo "")"
url_local="http://127.0.0.1:$PANDA_PORT"
url_lan="${lan:+http://$lan:$PANDA_PORT}"

open "$url_local"

if [ -z "${PANDA_QUIET:-}" ]; then
  if [ "${PANDA_MODE:-simulation}" = "live" ]; then
    till="Owner pin: $PANDA_OWNER_PIN"
  else
    till="Simulation: any pin opens the counter. Set PANDA_MODE=live in $PANDA_HOME/env for real USDC; the owner pin is then $PANDA_OWNER_PIN."
  fi
  msg="Guests on this wifi open:"$'\n'"${url_lan:-$url_local}"$'\n\n'"$till"$'\n\n'"Keep this Mac awake and on the cafe wifi. Quit from the menu bar or Activity Monitor (panda)."
  osascript -e 'display dialog "'"${msg//\"/\\\"}"'" with title "Causewaybay Panda is open" buttons {"OK"} default button "OK" with icon note' >/dev/null 2>&1 || true
fi

# Stay alive as the app process so Launchpad shows it running; stop the shop
# when the app is quit.
trap 'kill "$pid" 2>/dev/null; rm -f "$PANDA_HOME/panda.pid"; exit 0' TERM INT
wait "$pid"
LAUNCH
chmod +x "$app/Contents/MacOS/launch"

echo "built  $app"
echo "run    open \"$app\""
echo "login  make mac-install    (starts with this Mac)"
