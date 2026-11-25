//go:build !windows

package main

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
)

func fatal(msg string) { fmt.Fprintln(os.Stderr, msg); os.Exit(1) }

func openWindow(url, data string) (*exec.Cmd, error) {
	for _, n := range []string{"google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge", "brave-browser",
		"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"} {
		if p, err := exec.LookPath(n); err == nil {
			cmd := exec.Command(p, appArgs(url, data)...)
			return cmd, cmd.Start()
		}
	}
	return nil, errors.New("no Chrome/Edge/Chromium found")
}
