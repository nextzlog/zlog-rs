#!/bin/bash
. scripts/setup.sh
command -v cargo-about > /dev/null || cargo install cargo-about --version 0.9.2 --locked --features cli
cargo about generate -c about.toml -m "apps/$APP/Cargo.toml" about.hbs -o THIRD-PARTY-NOTICES.html
case "$RUNNER_OS" in
macOS)
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release -p "$APP" --bin "$PROGRAM" --target $t
    cargo build --release -p "$APP" --bin zlog --target $t
  done
  NOTICES=THIRD-PARTY-NOTICES.html scripts/macos.sh "$NAME" "$PROGRAM" \
    "target/aarch64-apple-darwin/release/$PROGRAM" "target/x86_64-apple-darwin/release/$PROGRAM"
  ln -s /Applications dist/Applications
  for n in 1 2 3; do
    hdiutil create -volname "$NAME" -srcfolder dist -ov -format UDZO "$PROGRAM.dmg" && break
    [ $n = 3 ] && exit 1
    sleep 10
  done
  lipo -create -output "$PROGRAM-cli-macos" target/aarch64-apple-darwin/release/zlog target/x86_64-apple-darwin/release/zlog
  files="$PROGRAM.dmg $PROGRAM-cli-macos"
  ;;
Windows)
  cargo build --release -p "$APP" --bin "$PROGRAM"
  cargo build --release -p "$APP" --bin zlog
  cp "target/release/$PROGRAM.exe" "$PROGRAM.exe"
  cp target/release/zlog.exe "$PROGRAM-cli.exe"
  7z a "$PROGRAM-windows.zip" "$PROGRAM.exe" THIRD-PARTY-NOTICES.html
  files="$PROGRAM.exe $PROGRAM-windows.zip $PROGRAM-cli.exe"
  ;;
Linux)
  cargo build --release -p "$APP" --bin "$PROGRAM"
  cargo build --release -p "$APP" --bin zlog
  curl -fsSL -o appimagetool https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  [ "$(head -c 4 appimagetool | od -An -c | tr -d ' ')" = '177ELF' ] || { echo "appimagetool is not an ELF binary"; exit 1; }
  chmod +x appimagetool
  NOTICES=THIRD-PARTY-NOTICES.html APPIMAGE_EXTRACT_AND_RUN=1 APPIMAGETOOL=$PWD/appimagetool scripts/linux.sh "$NAME" "$PROGRAM"
  cp "dist/$PROGRAM-x86_64.AppImage" .
  cp target/release/zlog "$PROGRAM-cli-linux"
  files="$PROGRAM-x86_64.AppImage $PROGRAM-cli-linux THIRD-PARTY-NOTICES.html"
  ;;
esac
mkdir -p assets
for f in $files; do
  cp "$f" assets/
  if command -v sha256sum > /dev/null; then sha256sum "$f"; else shasum -a 256 "$f"; fi > "assets/$f.sha256"
done
