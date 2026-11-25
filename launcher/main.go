// Marbeat launcher: a small native app that serves Marbeat from a local cache and opens it in its own window.
//
//   - Starts a web server on 127.0.0.1 (stable port, so browser storage survives between runs).
//   - Downloads the app from the update page (manifest.json) into a local cache. Every file is checked against its
//     SHA-256 from the manifest. Big files (AI models) are fetched the first time they are used, then kept for offline use.
//   - On each start it re-reads the manifest, drops files that changed, and can replace itself with a newer launcher.
//   - Opens Edge / Chrome in "app mode" (no tabs, no address bar) with its own profile, and exits when the window closes.
package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// Version is the launcher's own version (set at build time with -ldflags "-X main.Version=N").
var Version = "1"

const defaultBase = "https://assets.molive6316.com/marbeat/"

type FileEntry struct {
	Path   string `json:"path"`
	Size   int64  `json:"size"`
	SHA256 string `json:"sha256"`
	Core   bool   `json:"core,omitempty"`
}
type Build struct {
	URL    string `json:"url"`
	SHA256 string `json:"sha256"`
}
type LauncherInfo struct {
	Version string           `json:"version"`
	Builds  map[string]Build `json:"builds"`
}
type Manifest struct {
	Version  string       `json:"version"`
	Built    string       `json:"built"`
	Launcher LauncherInfo `json:"launcher"`
	Files    []FileEntry  `json:"files"`
}

type App struct {
	base, data, appDir string
	client             *http.Client

	mu     sync.RWMutex
	cur    *Manifest
	local  *Manifest
	byPath map[string]FileEntry
	status Status

	ln      net.Listener
	locks   sync.Map // path -> *sync.Mutex
	bytesDL atomic.Int64
	port    int
	quit    chan struct{}
}

type Status struct {
	Phase           string `json:"phase"` // starting | checking | downloading | ready | offline | error | prefetching
	Message         string `json:"message"`
	Done            int    `json:"done"`
	Total           int    `json:"total"`
	Online          bool   `json:"online"`
	ContentVersion  string `json:"contentVersion"`
	LauncherVersion string `json:"launcherVersion"`
	Bytes           int64  `json:"bytes"`
	UpdateNote      string `json:"updateNote,omitempty"`
}

func envOr(k, d string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return d
}

func dataDir() string {
	if d := os.Getenv("MARBEAT_DATA"); d != "" {
		return d
	}
	if runtime.GOOS == "windows" {
		if d := os.Getenv("LOCALAPPDATA"); d != "" {
			return filepath.Join(d, "Marbeat")
		}
	}
	home, _ := os.UserHomeDir()
	if runtime.GOOS == "darwin" {
		return filepath.Join(home, "Library", "Application Support", "Marbeat")
	}
	return filepath.Join(home, ".local", "share", "marbeat")
}

func main() {
	base := envOr("MARBEAT_BASE", defaultBase)
	if !strings.HasSuffix(base, "/") {
		base += "/"
	}
	data := dataDir()
	if err := os.MkdirAll(filepath.Join(data, "app"), 0o755); err != nil {
		fatal("Marbeat couldn't create its folder:\n" + data + "\n\n" + err.Error())
	}
	if f, err := os.OpenFile(filepath.Join(data, "launcher.log"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644); err == nil {
		if st, _ := f.Stat(); st != nil && st.Size() > 1<<20 {
			f.Close()
			os.Truncate(filepath.Join(data, "launcher.log"), 0)
			f, _ = os.OpenFile(filepath.Join(data, "launcher.log"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
		}
		log.SetOutput(f)
	}
	log.Printf("---- Marbeat launcher v%s starting (%s/%s), base %s, data %s", Version, runtime.GOOS, runtime.GOARCH, base, data)
	if exe, err := os.Executable(); err == nil {
		os.Remove(exe + ".old") // left over from a self-update
	}

	a := &App{base: base, data: data, appDir: filepath.Join(data, "app"), quit: make(chan struct{}),
		client: &http.Client{Timeout: 0}, byPath: map[string]FileEntry{}}
	a.status = Status{Phase: "starting", Message: "Starting…", LauncherVersion: Version}

	if os.Getenv("MARBEAT_JUST_UPDATED") != "" {
		time.Sleep(700 * time.Millisecond) // let the old version finish exiting and free its port
	}
	// one instance: if Marbeat is already running, just open another window on it
	if p := findRunning(); p > 0 {
		log.Printf("already running on port %d, opening a window", p)
		if os.Getenv("MARBEAT_NO_BROWSER") == "" {
			if cmd, err := openWindow(fmt.Sprintf("http://127.0.0.1:%d/", p), data); err == nil {
				cmd.Wait()
			}
		}
		return
	}

	ln, port := listen()
	a.port = port
	a.ln = ln
	log.Printf("serving on http://127.0.0.1:%d/", port)
	go func() { log.Printf("server stopped: %v", http.Serve(ln, a)) }()
	go a.refresh()

	url := fmt.Sprintf("http://127.0.0.1:%d/", port)
	if os.Getenv("MARBEAT_NO_BROWSER") != "" {
		fmt.Println("MARBEAT_URL=" + url)
		<-a.quit
		return
	}
	cmd, err := openWindow(url, data)
	if err != nil {
		log.Printf("no app window: %v", err)
		fatal("Marbeat couldn't open a window.\n\nIt needs Microsoft Edge or Google Chrome.\n\n" + err.Error())
	}
	done := make(chan struct{})
	go func() { cmd.Wait(); close(done) }()
	select {
	case <-done:
		log.Printf("window closed, exiting")
	case <-a.quit:
		log.Printf("quit requested")
	}
}

// ---------- server ----------

func listen() (net.Listener, int) {
	if p, _ := strconv.Atoi(os.Getenv("MARBEAT_PORT")); p > 0 {
		if ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", p)); err == nil {
			return ln, p
		}
	}
	for p := 47831; p < 47841; p++ { // a stable port keeps the app's saved settings between runs
		if ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", p)); err == nil {
			return ln, p
		}
	}
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		fatal("Marbeat couldn't start its local server:\n" + err.Error())
	}
	return ln, ln.Addr().(*net.TCPAddr).Port
}

func findRunning() int {
	c := &http.Client{Timeout: 700 * time.Millisecond}
	ports := []int{}
	if p, _ := strconv.Atoi(os.Getenv("MARBEAT_PORT")); p > 0 {
		ports = append(ports, p)
	}
	for p := 47831; p < 47841; p++ {
		ports = append(ports, p)
	}
	for _, p := range ports {
		r, err := c.Get(fmt.Sprintf("http://127.0.0.1:%d/__marbeat/ping", p))
		if err != nil {
			continue
		}
		var m map[string]any
		json.NewDecoder(r.Body).Decode(&m)
		r.Body.Close()
		if m["app"] == "marbeat-launcher" {
			return p
		}
	}
	return 0
}

var mimeTypes = map[string]string{
	".html": "text/html; charset=utf-8", ".js": "text/javascript", ".mjs": "text/javascript", ".json": "application/json",
	".wasm": "application/wasm", ".css": "text/css", ".png": "image/png", ".jpg": "image/jpeg", ".svg": "image/svg+xml",
	".onnx": "application/octet-stream", ".data": "application/octet-stream", ".model": "application/octet-stream",
	".ico": "image/x-icon", ".npy": "application/octet-stream", ".md": "text/plain; charset=utf-8",
}

func (a *App) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	// only answer requests addressed to localhost (blocks DNS-rebinding tricks)
	h := r.Host
	if !(strings.HasPrefix(h, "127.0.0.1:") || strings.HasPrefix(h, "localhost:") || h == "127.0.0.1" || h == "localhost") {
		http.Error(w, "forbidden", http.StatusForbidden)
		return
	}
	// cross-origin isolation: lets the browser use multi-threaded WebAssembly (much faster AI)
	w.Header().Set("Cross-Origin-Opener-Policy", "same-origin")
	w.Header().Set("Cross-Origin-Embedder-Policy", "require-corp")
	w.Header().Set("Cross-Origin-Resource-Policy", "same-origin")
	p := path.Clean("/" + r.URL.Path)
	if strings.HasPrefix(p, "/__marbeat/") {
		a.api(w, r, strings.TrimPrefix(p, "/__marbeat/"))
		return
	}
	if p == "/" {
		p = "/index.html"
	}
	rel := strings.TrimPrefix(p, "/")
	if rel == "index.html" && !a.hasIndex() {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Header().Set("Cache-Control", "no-store")
		io.WriteString(w, bootPage)
		return
	}
	e, ok := a.entry(rel)
	if !ok {
		http.NotFound(w, r)
		return
	}
	if err := a.ensure(e); err != nil {
		log.Printf("ensure %s: %v", rel, err)
		w.Header().Set("Content-Type", "text/plain; charset=utf-8")
		msg := "Marbeat couldn't get '" + rel + "'. It isn't saved yet and the update page can't be reached (" + err.Error() + "). Connect to the internet once so it can be saved."
		if strings.Contains(err.Error(), "integrity") || strings.Contains(err.Error(), "expected") {
			msg = "Marbeat refused '" + rel + "': the downloaded copy didn't match the update page's checksum, so it was thrown away (" + err.Error() + ")."
		}
		http.Error(w, msg, http.StatusServiceUnavailable)
		return
	}
	f, err := os.Open(filepath.Join(a.appDir, filepath.FromSlash(rel)))
	if err != nil {
		http.NotFound(w, r)
		return
	}
	defer f.Close()
	st, _ := f.Stat()
	if t, ok := mimeTypes[strings.ToLower(path.Ext(rel))]; ok {
		w.Header().Set("Content-Type", t)
	}
	w.Header().Set("ETag", `"`+e.SHA256+`"`)
	w.Header().Set("Cache-Control", "no-cache") // always revalidate (cheap: ETag), so updates show up immediately
	http.ServeContent(w, r, rel, st.ModTime(), f)
}

func (a *App) api(w http.ResponseWriter, r *http.Request, name string) {
	w.Header().Set("Cache-Control", "no-store")
	switch name {
	case "ping":
		a.mu.RLock()
		cv := ""
		if a.cur != nil {
			cv = a.cur.Version
		} else if a.local != nil {
			cv = a.local.Version
		}
		a.mu.RUnlock()
		json.NewEncoder(w).Encode(map[string]any{"app": "marbeat-launcher", "launcher": Version, "content": cv, "os": runtime.GOOS})
	case "status":
		json.NewEncoder(w).Encode(a.snapshot())
	case "refresh":
		go a.refresh()
		json.NewEncoder(w).Encode(a.snapshot())
	case "prefetch":
		go a.prefetchAll()
		json.NewEncoder(w).Encode(a.snapshot())
	case "quit":
		json.NewEncoder(w).Encode(map[string]bool{"ok": true})
		go func() { time.Sleep(200 * time.Millisecond); close(a.quit) }()
	default:
		http.NotFound(w, r)
	}
}

func (a *App) snapshot() Status {
	a.mu.RLock()
	defer a.mu.RUnlock()
	s := a.status
	s.Bytes = a.bytesDL.Load()
	return s
}
func (a *App) setStatus(f func(s *Status)) {
	a.mu.Lock()
	f(&a.status)
	a.mu.Unlock()
}

func (a *App) entry(rel string) (FileEntry, bool) {
	a.mu.RLock()
	defer a.mu.RUnlock()
	e, ok := a.byPath[rel]
	return e, ok
}
func (a *App) hasIndex() bool {
	e, ok := a.entry("index.html")
	if !ok {
		return false
	}
	return a.cached(e)
}

func (a *App) cached(e FileEntry) bool {
	st, err := os.Stat(filepath.Join(a.appDir, filepath.FromSlash(e.Path)))
	return err == nil && st.Size() == e.Size
}

// ---------- updating ----------

func (a *App) manifestPath() string { return filepath.Join(a.data, "manifest.json") }

func (a *App) loadLocal() {
	b, err := os.ReadFile(a.manifestPath())
	if err != nil {
		return
	}
	var m Manifest
	if json.Unmarshal(b, &m) == nil && len(m.Files) > 0 {
		a.mu.Lock()
		a.local = &m
		a.cur = &m
		a.byPath = index(&m)
		a.mu.Unlock()
	}
}
func index(m *Manifest) map[string]FileEntry {
	r := map[string]FileEntry{}
	for _, f := range m.Files {
		r[f.Path] = f
	}
	return r
}

func (a *App) fetchManifest() (*Manifest, error) {
	c := &http.Client{Timeout: 12 * time.Second}
	r, err := c.Get(a.base + "manifest.json?t=" + strconv.FormatInt(time.Now().Unix(), 10))
	if err != nil {
		return nil, err
	}
	defer r.Body.Close()
	if r.StatusCode != 200 {
		return nil, fmt.Errorf("update page answered %d", r.StatusCode)
	}
	var m Manifest
	if err := json.NewDecoder(io.LimitReader(r.Body, 8<<20)).Decode(&m); err != nil {
		return nil, err
	}
	if len(m.Files) == 0 {
		return nil, errors.New("manifest has no files")
	}
	for _, f := range m.Files { // never trust a path that could escape the cache folder
		if strings.Contains(f.Path, "..") || strings.HasPrefix(f.Path, "/") || strings.Contains(f.Path, "\\") {
			return nil, errors.New("bad path in manifest: " + f.Path)
		}
	}
	return &m, nil
}

func (a *App) refresh() {
	a.loadLocalOnce()
	a.setStatus(func(s *Status) { s.Phase = "checking"; s.Message = "Checking for updates…" })
	m, err := a.fetchManifest()
	if err != nil {
		log.Printf("manifest: %v", err)
		a.mu.RLock()
		have := a.local != nil
		a.mu.RUnlock()
		if have {
			a.setStatus(func(s *Status) {
				s.Phase = "offline"
				s.Online = false
				s.Message = "Offline: using your saved copy"
				s.ContentVersion = a.local.Version
			})
		} else {
			a.setStatus(func(s *Status) {
				s.Phase = "error"
				s.Message = "Can't reach the update page (" + a.base + "manifest.json). Check your internet connection and try again: " + err.Error()
			})
		}
		return
	}
	// newer launcher? replace ourselves and restart
	if a.maybeSelfUpdate(m) {
		return
	}
	// drop cached files that changed or were removed
	a.mu.Lock()
	changed := 0
	if a.local != nil {
		newIdx := index(m)
		for _, old := range a.local.Files {
			if ne, ok := newIdx[old.Path]; !ok || ne.SHA256 != old.SHA256 {
				os.Remove(filepath.Join(a.appDir, filepath.FromSlash(old.Path)))
				changed++
			}
		}
	}
	a.cur = m
	a.byPath = index(m)
	a.mu.Unlock()
	// download the small "core" files now (everything else is fetched the first time it is used)
	var need []FileEntry
	for _, f := range m.Files {
		if (f.Core || f.Path == "index.html") && !a.cached(f) {
			need = append(need, f)
		}
	}
	a.setStatus(func(s *Status) {
		s.Phase = "downloading"
		s.Online = true
		s.Total = len(need)
		s.Done = 0
		s.ContentVersion = m.Version
		s.Message = "Getting Marbeat…"
	})
	if len(need) > 0 {
		if err := a.downloadAll(need, func() { a.setStatus(func(s *Status) { s.Done++ }) }); err != nil {
			log.Printf("core download: %v", err)
			if !a.hasIndex() {
				a.setStatus(func(s *Status) { s.Phase = "error"; s.Message = "Couldn't download Marbeat: " + err.Error() })
				return
			}
		}
	}
	a.saveManifest(m)
	a.mu.Lock()
	a.local = m
	a.mu.Unlock()
	note := ""
	if changed > 0 {
		note = fmt.Sprintf("Updated: %d file(s) changed", changed)
	}
	a.setStatus(func(s *Status) {
		s.Phase = "ready"
		s.Online = true
		s.Message = "Ready"
		s.ContentVersion = m.Version
		s.UpdateNote = note
	})
	log.Printf("ready, content %s (%d files, %d changed)", m.Version, len(m.Files), changed)
}

var loadOnce sync.Once

func (a *App) loadLocalOnce() { loadOnce.Do(a.loadLocal) }

func (a *App) saveManifest(m *Manifest) {
	b, _ := json.Marshal(m)
	tmp := a.manifestPath() + ".tmp"
	if os.WriteFile(tmp, b, 0o644) == nil {
		os.Rename(tmp, a.manifestPath())
	}
}

func (a *App) downloadAll(files []FileEntry, each func()) error {
	var wg sync.WaitGroup
	sem := make(chan struct{}, 4)
	var first error
	var emu sync.Mutex
	for _, f := range files {
		f := f
		wg.Add(1)
		sem <- struct{}{}
		go func() {
			defer wg.Done()
			defer func() { <-sem }()
			if err := a.ensure(f); err != nil {
				emu.Lock()
				if first == nil {
					first = err
				}
				emu.Unlock()
			} else if each != nil {
				each()
			}
		}()
	}
	wg.Wait()
	return first
}

// "Download everything for offline use"
func (a *App) prefetchAll() {
	a.mu.RLock()
	m := a.cur
	a.mu.RUnlock()
	if m == nil {
		return
	}
	var need []FileEntry
	for _, f := range m.Files {
		if !a.cached(f) {
			need = append(need, f)
		}
	}
	a.setStatus(func(s *Status) {
		s.Phase = "prefetching"
		s.Total = len(need)
		s.Done = 0
		s.Message = "Downloading everything for offline use…"
	})
	err := a.downloadAll(need, func() { a.setStatus(func(s *Status) { s.Done++ }) })
	a.setStatus(func(s *Status) {
		s.Phase = "ready"
		if err != nil {
			s.Message = "Some files couldn't be downloaded: " + err.Error()
		} else {
			s.Message = "Everything is saved for offline use"
		}
	})
}

// make sure one file is in the cache, downloading and verifying it if needed
func (a *App) ensure(e FileEntry) error {
	if a.cached(e) {
		return nil
	}
	v, _ := a.locks.LoadOrStore(e.Path, &sync.Mutex{})
	mu := v.(*sync.Mutex)
	mu.Lock()
	defer mu.Unlock()
	if a.cached(e) {
		return nil
	}
	dest := filepath.Join(a.appDir, filepath.FromSlash(e.Path))
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		return err
	}
	var err error
	for try := 0; try < 3; try++ {
		if err = a.download(&url.URL{Path: e.Path}, e.SHA256, e.Size, dest); err == nil {
			return nil
		}
		time.Sleep(time.Duration(try+1) * 700 * time.Millisecond)
	}
	return err
}

func (a *App) download(rel *url.URL, wantSHA string, wantSize int64, dest string) error {
	u := a.base + strings.TrimPrefix(rel.EscapedPath(), "/")
	r, err := a.client.Get(u)
	if err != nil {
		return err
	}
	defer r.Body.Close()
	if r.StatusCode != 200 {
		return fmt.Errorf("%s answered %d", u, r.StatusCode)
	}
	tmp := dest + ".part"
	f, err := os.Create(tmp)
	if err != nil {
		return err
	}
	h := sha256.New()
	n, err := io.Copy(io.MultiWriter(f, h, countWriter{&a.bytesDL}), r.Body)
	f.Close()
	if err != nil {
		os.Remove(tmp)
		return err
	}
	if wantSize > 0 && n != wantSize {
		os.Remove(tmp)
		return fmt.Errorf("%s: got %d bytes, expected %d", u, n, wantSize)
	}
	if got := hex.EncodeToString(h.Sum(nil)); wantSHA != "" && got != wantSHA {
		os.Remove(tmp)
		return fmt.Errorf("%s failed its integrity check (hash mismatch)", u)
	}
	return os.Rename(tmp, dest)
}

type countWriter struct{ n *atomic.Int64 }

func (c countWriter) Write(p []byte) (int, error) { c.n.Add(int64(len(p))); return len(p), nil }

// ---------- updating the launcher itself ----------

func verNum(s string) int { n, _ := strconv.Atoi(strings.TrimSpace(s)); return n }

func (a *App) maybeSelfUpdate(m *Manifest) bool {
	if verNum(m.Launcher.Version) <= verNum(Version) {
		return false
	}
	if os.Getenv("MARBEAT_JUST_UPDATED") != "" {
		log.Printf("already tried to self-update this run; skipping")
		return false
	}
	b, ok := m.Launcher.Builds[runtime.GOOS+"-"+runtime.GOARCH]
	if !ok || b.URL == "" {
		return false
	}
	a.setStatus(func(s *Status) { s.Phase = "downloading"; s.Message = "Updating the Marbeat app…" })
	exe, err := os.Executable()
	if err != nil {
		return false
	}
	exe, _ = filepath.EvalSymlinks(exe)
	newPath := exe + ".new"
	u, err := url.Parse(b.URL)
	if err != nil {
		return false
	}
	full := a.base + strings.TrimPrefix(u.EscapedPath(), "/")
	if u.IsAbs() {
		full = b.URL
	}
	log.Printf("self-update %s -> %s from %s", Version, m.Launcher.Version, full)
	r, err := a.client.Get(full)
	if err != nil || r.StatusCode != 200 {
		log.Printf("self-update download failed: %v", err)
		return false
	}
	defer r.Body.Close()
	f, err := os.OpenFile(newPath, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o755)
	if err != nil {
		log.Printf("self-update: can't write next to the app: %v", err)
		return false
	}
	h := sha256.New()
	_, err = io.Copy(io.MultiWriter(f, h), r.Body)
	f.Close()
	if err != nil || (b.SHA256 != "" && hex.EncodeToString(h.Sum(nil)) != b.SHA256) {
		os.Remove(newPath)
		log.Printf("self-update: bad download (%v)", err)
		return false
	}
	old := exe + ".old"
	os.Remove(old)
	if err := os.Rename(exe, old); err != nil { // a running .exe can be renamed on Windows, just not overwritten
		os.Remove(newPath)
		log.Printf("self-update: can't move the old app: %v", err)
		return false
	}
	if err := os.Rename(newPath, exe); err != nil {
		os.Rename(old, exe)
		log.Printf("self-update: can't place the new app: %v", err)
		return false
	}
	if a.ln != nil {
		a.ln.Close() // free the port first, so the new version doesn't mistake us for a running copy
	}
	cmd := exec.Command(exe, os.Args[1:]...)
	cmd.Env = append(os.Environ(), "MARBEAT_JUST_UPDATED=1")
	if err := cmd.Start(); err != nil {
		log.Printf("self-update: restart failed: %v", err)
		return false
	}
	log.Printf("self-update done, restarting")
	time.Sleep(300 * time.Millisecond)
	os.Exit(0)
	return true
}

const bootPage = `<!doctype html><meta charset="utf-8"><title>Marbeat</title><meta name="viewport" content="width=device-width,initial-scale=1">
<style>body{margin:0;background:#0b0b12;color:#f2f2f7;font:16px system-ui,sans-serif;display:grid;place-items:center;height:100vh;text-align:center}
.b{font-weight:800;font-size:30px}.b i{color:#ff4d8d;font-style:normal}#bar{width:280px;height:8px;background:#1b1b2a;border-radius:6px;overflow:hidden;margin:18px auto}
#bar div{height:100%;width:0;background:linear-gradient(90deg,#ff4d8d,#6c5cff);transition:width .3s}#m{color:#8b8ba3;max-width:420px;margin:0 auto;padding:0 18px}button{background:#1b1b2a;border:1px solid #333;color:#fff;border-radius:10px;padding:10px 16px;cursor:pointer;margin-top:14px}</style>
<div><div class="b">Mar<i>beat</i></div><div id="bar"><div id="f"></div></div><div id="m">Starting…</div><button id="r" style="display:none" onclick="fetch('/__marbeat/refresh');this.style.display='none'">Try again</button></div>
<script>
async function tick(){try{const s=await (await fetch('/__marbeat/status',{cache:'no-store'})).json();
 document.getElementById('m').textContent=s.message+(s.total?' ('+s.done+'/'+s.total+')':'');
 document.getElementById('f').style.width=(s.total?Math.round(s.done/s.total*100):8)+'%';
 document.getElementById('r').style.display=s.phase==='error'?'inline-block':'none';
 if(s.phase==='ready'||s.phase==='offline'){location.reload();return}}catch(e){}
 setTimeout(tick,450)}tick();
</script>`
