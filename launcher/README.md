# Marbeat for Windows (the launcher)

`marbeat/download/Marbeat.exe` is a small native app (about 6 MB, written in Go, no installer, no admin rights).
Double-click it and Marbeat opens in its own window (Edge or Chrome in "app mode": no tabs, no address bar).

## How it works
1. It starts a tiny web server on `127.0.0.1` (port 47831, so your saved settings survive between runs).
2. It reads the **update page**, `https://assets.molive6316.com/marbeat/manifest.json`: every file of the web app with its size and SHA-256.
3. It downloads only what changed. The page itself comes first (instant start); big files (AI models) are fetched **the first time a feature uses them**
   and then kept. A button in the app ("Save everything for offline use") fetches the lot.
4. Every file is verified against its SHA-256; a file that doesn't match is thrown away, never served.
5. Offline? It serves the saved copy. Files never downloaded show a clear message instead.
6. If `manifest.json` lists a **newer launcher** (`launcher.version`), the app downloads it, checks its hash, replaces itself and restarts.
7. Because it serves the page itself with cross-origin-isolation headers, the AI parts can use **multiple CPU threads** (about 2-3x faster separation/cloning than the plain website).

Everything lives in `%LOCALAPPDATA%\Marbeat` (cache in `app\`, browser profile in `profile\`, `launcher.log`). To uninstall: delete `Marbeat.exe` and that folder.

## What the app saves (the library)
The web page talks to the launcher on `/__marbeat/api/` (needs a per-run token; requests from other websites are refused):
- **Characters, tournaments, settings** -> `%LOCALAPPDATA%\Marbeat\library\store\*.json`
- **Voice samples** (for cloned voices) -> `...\library\blobs\`
- **Finished videos** -> `%USERPROFILE%\Videos\Marbeat\<tournament name>\Round1-Heat2-Poppy-Milo.mp4` (never overwrites; names are made safe for Windows)
On the plain website the same features work, saved in the browser's own storage instead.

## Characters and tournaments
- **Characters** keep name, colour, voice (preset or your own cloned sample) and a win record (races, wins, podiums, titles).
- **Tournaments:** pick characters, marbles per heat (2-8), how many advance, map. Heats are raced for real; results come from the physics.
  Rounds are built automatically until one final heat; the winner is champion. **Run all remaining heats** renders and saves every heat video unattended;
  Stop halts it, and pressing Run again resumes where it stopped.

## Shipping an update (what you do)
```bash
# 1. change anything in marbeat/
./tools/build-launcher.sh          # rebuilds Marbeat.exe and regenerates marbeat/manifest.json
# 2. copy marbeat/ into the assets repo and push (the site assets.molive6316.com serves it)
```
Users get the new files the next time they start the app, or when they press **Check for updates** in the app.
To ship a **new launcher**, bump `launcher/VERSION` first (it is a whole number), then run the script.
`tools/make-manifest.py` alone regenerates just the manifest.

## Environment variables (for testing)
`MARBEAT_BASE` (update page URL), `MARBEAT_DATA` (data folder), `MARBEAT_PORT`, `MARBEAT_NO_BROWSER=1` (don't open a window; prints the URL).

## Known limits
- **Not code-signed.** Windows SmartScreen shows "Windows protected your PC": click *More info -> Run anyway*. Signing needs a paid certificate.
- Needs Microsoft Edge (built into Windows 10/11) or Chrome.
- The `.exe` uses Windows' default icon (embedding one needs an extra build tool that isn't included).
- Built and tested on Linux against a stand-in update page; the Windows-only parts (finding Edge/Chrome, the error dialog) are small and **unverified on a real Windows PC**.
