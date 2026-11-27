#!/bin/sh
# Build the Windows exe, copy it into the assets checkout and write native.json. Usage: release-native.sh "release notes"
# Bump VERSION in native/app/src/update.rs first.
set -e
cd "$(dirname "$0")/../native"
echo "running tests..."
cargo test --release -p marbeat-core -p marbeat || { echo "TESTS FAILED, not releasing"; exit 1; }
cargo zigbuild --release --target x86_64-pc-windows-gnu -p marbeat
OUT=${ASSETS_DIR:-../../assets}/marbeat
EXE=target/x86_64-pc-windows-gnu/release/marbeat.exe
cp "$EXE" "$OUT/download/MarbeatNative.exe"
V=$(sed -n 's/.*VERSION: u32 = \([0-9]*\);.*/\1/p' app/src/update.rs)
H=$(sha256sum "$EXE" | cut -d' ' -f1)
printf '{"version":%s,"notes":"%s","url":"download/MarbeatNative.exe","sha256":"%s"}\n' "$V" "${1:-}" "$H" > "$OUT/native.json"
echo "released v$V $H"
