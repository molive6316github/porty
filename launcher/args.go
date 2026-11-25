package main

import "path/filepath"

// Edge/Chrome "app mode": a bare window with no tabs or address bar, and its own profile folder.
func appArgs(url, data string) []string {
	return []string{
		"--app=" + url,
		"--user-data-dir=" + filepath.Join(data, "profile"),
		"--no-first-run", "--no-default-browser-check",
		"--window-size=1360,900",
		"--disable-features=Translate,msEdgeShopping",
	}
}
