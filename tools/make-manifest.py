#!/usr/bin/env python3
"""Writes marbeat/manifest.json: the page the Marbeat app fetches to find updates.

For every file the web app serves it records path, size and SHA-256. The app downloads only files whose hash changed.
Run this after changing anything in marbeat/ (build-launcher.sh does it for you).  Usage: make-manifest.py [marbeat-dir]
"""
import hashlib, json, os, sys, time

root = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), '..', 'marbeat'))
launcher_dir = os.path.join(os.path.dirname(__file__), '..', 'launcher')
EXCLUDE_DIRS = {'download'}                      # the installers themselves are not part of the web app
EXCLUDE_FILES = {'manifest.json', 'README.md', '.DS_Store'}
CORE = {'index.html'}                            # downloaded immediately; everything else is fetched when first used

def sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()

files = []
for dp, dn, fn in os.walk(root):
    dn[:] = sorted(d for d in dn if not (dp == root and d in EXCLUDE_DIRS) and not d.startswith('.'))
    for name in sorted(fn):
        if name in EXCLUDE_FILES or name.startswith('.') or name.endswith('.part'):
            continue
        full = os.path.join(dp, name)
        rel = os.path.relpath(full, root).replace(os.sep, '/')
        e = {'path': rel, 'size': os.path.getsize(full), 'sha256': sha(full)}
        if rel in CORE:
            e['core'] = True
        files.append(e)

content_version = hashlib.sha256('\n'.join(f"{f['path']}:{f['sha256']}" for f in files).encode()).hexdigest()[:12]

builds = {}
for key, rel in {'windows-amd64': 'download/Marbeat.exe'}.items():
    p = os.path.join(root, rel)
    if os.path.exists(p):
        builds[key] = {'url': rel, 'sha256': sha(p)}
try:
    lver = open(os.path.join(launcher_dir, 'VERSION')).read().strip()
except OSError:
    lver = '1'

manifest = {
    'version': content_version,
    'built': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
    'launcher': {'version': lver, 'builds': builds},
    'files': files,
}
with open(os.path.join(root, 'manifest.json'), 'w') as f:
    json.dump(manifest, f, indent=1)
total = sum(x['size'] for x in files)
print(f"manifest.json: content {content_version}, launcher v{lver}, {len(files)} files, {total/1e6:.0f} MB, builds: {', '.join(builds) or 'none'}")
