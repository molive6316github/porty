//go:build windows

package main

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"syscall"
	"unsafe"
)

func fatal(msg string) {
	user32 := syscall.NewLazyDLL("user32.dll")
	box := user32.NewProc("MessageBoxW")
	t, _ := syscall.UTF16PtrFromString("Marbeat")
	m, _ := syscall.UTF16PtrFromString(msg)
	box.Call(0, uintptr(unsafe.Pointer(m)), uintptr(unsafe.Pointer(t)), 0x10)
	os.Exit(1)
}

func browserCandidates() []string {
	var out []string
	for _, env := range []string{"ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"} {
		root := os.Getenv(env)
		if root == "" {
			continue
		}
		out = append(out,
			filepath.Join(root, `Microsoft\Edge\Application\msedge.exe`),
			filepath.Join(root, `Google\Chrome\Application\chrome.exe`),
			filepath.Join(root, `BraveSoftware\Brave-Browser\Application\brave.exe`),
			filepath.Join(root, `Chromium\Application\chrome.exe`),
		)
	}
	return out
}

func openWindow(url, data string) (*exec.Cmd, error) {
	for _, p := range browserCandidates() {
		if _, err := os.Stat(p); err == nil {
			return startApp(p, url, data)
		}
	}
	for _, n := range []string{"msedge.exe", "chrome.exe"} {
		if p, err := exec.LookPath(n); err == nil {
			return startApp(p, url, data)
		}
	}
	return nil, errors.New("no Edge or Chrome found")
}

func startApp(exe, url, data string) (*exec.Cmd, error) {
	cmd := exec.Command(exe, appArgs(url, data)...)
	cmd.SysProcAttr = &syscall.SysProcAttr{}
	return cmd, cmd.Start()
}
