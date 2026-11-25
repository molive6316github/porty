#!/usr/bin/env bash
# Builds the Windows app (marbeat/download/Marbeat.exe) and refreshes marbeat/manifest.json.
# To ship a NEW launcher: bump launcher/VERSION first. Users' apps replace themselves on their next start.
set -euo pipefail
cd "$(dirname "$0")/.."
V=$(cat launcher/VERSION)
mkdir -p marbeat/download
(cd launcher && CGO_ENABLED=0 GOOS=windows GOARCH=amd64 go build -trimpath -ldflags "-s -w -H=windowsgui -X main.Version=$V" -o ../marbeat/download/Marbeat.exe .)
echo "built Marbeat.exe v$V ($(du -h marbeat/download/Marbeat.exe | cut -f1))"
python3 tools/make-manifest.py marbeat
