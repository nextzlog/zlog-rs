#!/bin/sh
# Wraps release builds of zlog-gui into zLog.app, so that macOS starts it without a terminal.
# Usage: scripts/macos-app.sh [binary ...] (default: target/release/zlog-gui); output in dist/.
set -eu
cd "$(dirname "$0")/.."
[ $# -gt 0 ] || set -- target/release/zlog-gui
app=dist/zLog.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
if [ $# -gt 1 ]; then
  lipo -create -output "$app/Contents/MacOS/zlog-gui" "$@"
else
  cp "$1" "$app/Contents/MacOS/zlog-gui"
fi
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>zLog RS</string>
  <key>CFBundleIdentifier</key><string>dev.nextzlog.zlog-rs</string>
  <key>CFBundleExecutable</key><string>zlog-gui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>5.0.0</string>
  <key>NSMicrophoneUsageDescription</key><string>zLog decodes Morse code from the audio input.</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$app"
echo "$app"
