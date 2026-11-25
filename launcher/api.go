package main

// Library API: lets the web app keep characters, tournaments, voice samples and finished videos on disk.
// Everything is under /__marbeat/api/ and needs a per-run token (so other websites can't touch your files).

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
)

var tokenHex = func() string { b := make([]byte, 16); rand.Read(b); return hex.EncodeToString(b) }()

var keyRe = regexp.MustCompile(`^[a-z0-9_-]{1,40}$`)
var idRe = regexp.MustCompile(`^[A-Za-z0-9_-]{1,64}$`)

func exportDir() string {
	if v := os.Getenv("MARBEAT_EXPORT"); v != "" {
		return v
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, "Videos", "Marbeat")
}

func (a *App) libDir(sub string) string {
	d := filepath.Join(a.data, "library", sub)
	os.MkdirAll(d, 0o755)
	return d
}

func writeJSON(w http.ResponseWriter, code int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	json.NewEncoder(w).Encode(v)
}

func (a *App) authorized(r *http.Request) bool {
	if r.Header.Get("X-Marbeat-Token") != tokenHex {
		return false
	}
	// browsers tell us where a request came from; refuse anything from another site
	if s := r.Header.Get("Sec-Fetch-Site"); s != "" && s != "same-origin" && s != "none" {
		return false
	}
	return true
}

// make a safe single file/folder name: no paths, no characters Windows forbids, no reserved names
func safeName(s, fallback string) string {
	s = strings.Map(func(r rune) rune {
		if r < 32 || strings.ContainsRune(`<>:"/\|?*`, r) {
			return '_'
		}
		return r
	}, s)
	s = strings.Trim(strings.TrimSpace(s), ". ")
	if s == "" || s == "." || s == ".." {
		return fallback
	}
	up := strings.ToUpper(strings.TrimSuffix(s, filepath.Ext(s)))
	switch up {
	case "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3":
		s = "_" + s
	}
	if len(s) > 120 {
		s = s[:120]
	}
	return s
}

func uniquePath(dir, name string) string {
	ext := filepath.Ext(name)
	base := strings.TrimSuffix(name, ext)
	p := filepath.Join(dir, name)
	for i := 1; ; i++ {
		if _, err := os.Stat(p); os.IsNotExist(err) {
			return p
		}
		p = filepath.Join(dir, fmt.Sprintf("%s-%d%s", base, i, ext))
	}
}

func (a *App) apiV2(w http.ResponseWriter, r *http.Request, rest string) {
	w.Header().Set("Cache-Control", "no-store")
	if rest == "token" && r.Method == http.MethodGet {
		if s := r.Header.Get("Sec-Fetch-Site"); s != "" && s != "same-origin" && s != "none" {
			http.Error(w, "forbidden", http.StatusForbidden)
			return
		}
		writeJSON(w, 200, map[string]string{"token": tokenHex})
		return
	}
	if !a.authorized(r) {
		http.Error(w, "forbidden", http.StatusForbidden)
		return
	}
	parts := strings.SplitN(rest, "/", 2)
	arg := ""
	if len(parts) > 1 {
		arg = parts[1]
	}
	switch parts[0] {
	case "info":
		_, ffErr := exec.LookPath("ffmpeg")
		writeJSON(w, 200, map[string]any{"exportDir": exportDir(), "os": runtime.GOOS, "launcher": Version, "ffmpeg": ffErr == nil, "dataDir": a.data})
	case "store":
		if !keyRe.MatchString(arg) {
			http.Error(w, "bad key", 400)
			return
		}
		p := filepath.Join(a.libDir("store"), arg+".json")
		switch r.Method {
		case http.MethodGet:
			b, err := os.ReadFile(p)
			w.Header().Set("Content-Type", "application/json")
			if err != nil {
				io.WriteString(w, `{"value":null}`)
				return
			}
			io.WriteString(w, `{"value":`)
			w.Write(b)
			io.WriteString(w, `}`)
		case http.MethodPut:
			b, err := io.ReadAll(http.MaxBytesReader(w, r.Body, 16<<20))
			if err != nil || !json.Valid(b) {
				http.Error(w, "bad json", 400)
				return
			}
			tmp := p + ".tmp"
			if err := os.WriteFile(tmp, b, 0o644); err != nil || os.Rename(tmp, p) != nil {
				http.Error(w, "couldn't save", 500)
				return
			}
			writeJSON(w, 200, map[string]bool{"ok": true})
		default:
			http.Error(w, "method", 405)
		}
	case "blob":
		if !idRe.MatchString(arg) {
			http.Error(w, "bad id", 400)
			return
		}
		p := filepath.Join(a.libDir("blobs"), arg)
		switch r.Method {
		case http.MethodGet:
			f, err := os.Open(p)
			if err != nil {
				http.NotFound(w, r)
				return
			}
			defer f.Close()
			st, _ := f.Stat()
			w.Header().Set("Content-Type", "application/octet-stream")
			http.ServeContent(w, r, arg, st.ModTime(), f)
		case http.MethodPut:
			tmp := p + ".tmp"
			f, err := os.Create(tmp)
			if err != nil {
				http.Error(w, "couldn't save", 500)
				return
			}
			_, err = io.Copy(f, http.MaxBytesReader(w, r.Body, 256<<20))
			f.Close()
			if err != nil || os.Rename(tmp, p) != nil {
				os.Remove(tmp)
				http.Error(w, "couldn't save", 500)
				return
			}
			writeJSON(w, 200, map[string]bool{"ok": true})
		case http.MethodDelete:
			os.Remove(p)
			writeJSON(w, 200, map[string]bool{"ok": true})
		default:
			http.Error(w, "method", 405)
		}
	case "export": // save a finished video straight into the Marbeat folder (optionally inside a sub-folder)
		if r.Method != http.MethodPost {
			http.Error(w, "method", 405)
			return
		}
		name := safeName(r.URL.Query().Get("name"), "marbeat.mp4")
		dir := exportDir()
		if f := strings.TrimSpace(r.URL.Query().Get("folder")); f != "" {
			dir = filepath.Join(dir, safeName(f, "videos"))
		}
		if err := os.MkdirAll(dir, 0o755); err != nil {
			http.Error(w, "can't create folder: "+err.Error(), 500)
			return
		}
		dest := uniquePath(dir, name)
		tmp := dest + ".part"
		f, err := os.Create(tmp)
		if err != nil {
			http.Error(w, "can't write: "+err.Error(), 500)
			return
		}
		n, err := io.Copy(f, http.MaxBytesReader(w, r.Body, 4<<30))
		f.Close()
		if err != nil || os.Rename(tmp, dest) != nil {
			os.Remove(tmp)
			http.Error(w, "couldn't save the video", 500)
			return
		}
		writeJSON(w, 200, map[string]any{"path": dest, "bytes": n, "dir": dir})
	case "open": // show a saved file in Explorer / Finder (only inside the Marbeat folder)
		var body struct {
			Path string `json:"path"`
		}
		if json.NewDecoder(io.LimitReader(r.Body, 4096)).Decode(&body) != nil || body.Path == "" {
			http.Error(w, "bad request", 400)
			return
		}
		root, _ := filepath.Abs(exportDir())
		p, _ := filepath.Abs(body.Path)
		if rel, err := filepath.Rel(root, p); err != nil || strings.HasPrefix(rel, "..") {
			http.Error(w, "outside the Marbeat folder", 403)
			return
		}
		switch runtime.GOOS {
		case "windows":
			exec.Command("explorer.exe", "/select,"+p).Start()
		case "darwin":
			exec.Command("open", "-R", p).Start()
		default:
			exec.Command("xdg-open", filepath.Dir(p)).Start()
		}
		writeJSON(w, 200, map[string]bool{"ok": true})
	default:
		http.NotFound(w, r)
	}
}
