//! Offline HD export: render every frame with the pro finish, pipe raw RGBA to ffmpeg, mux the song in.
use crate::{audio::Song, render::*, sim::FPS};
use std::io::Write;
use std::process::{Command, Stdio};

fn works(p: &str) -> bool {
    Command::new(p).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}
fn cache_file() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").map(std::path::PathBuf::from).or_else(|_| std::env::var("HOME").map(|h| std::path::Path::new(&h).join(".config"))).unwrap_or_else(|_| ".".into());
    base.join("Marbeat").join("ffmpeg-path.txt")
}
/// Depth-limited scan for an ffmpeg binary. Skips huge/irrelevant trees and stops at `deadline`.
fn scan(dir: &std::path::Path, exe: &str, depth: u32, deadline: std::time::Instant, out: &mut Vec<std::path::PathBuf>) {
    if depth == 0 || std::time::Instant::now() > deadline { return; }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut subs = vec![];
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        if ft.is_file() { if name.eq_ignore_ascii_case(exe) { out.push(e.path()); } }
        else if ft.is_dir() && !ft.is_symlink() {
            let l = name.to_lowercase();
            if matches!(l.as_str(), "windows" | "$recycle.bin" | "system volume information" | "node_modules" | ".git" | "proc" | "sys" | "dev" | "appdata\\locallow") || l.starts_with('.') && l != ".local" { continue; }
            subs.push(e.path());
        }
    }
    // shallow first: bin/ folders and anything that looks like ffmpeg before the rest
    subs.sort_by_key(|p| { let l = p.to_string_lossy().to_lowercase(); if l.contains("ffmpeg") || l.ends_with("bin") { 0 } else { 1 } });
    for s in subs { scan(&s, exe, depth - 1, deadline, out); }
}
static OVERRIDE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
pub fn set_ffmpeg_override(p: Option<String>) { *OVERRIDE.lock().unwrap() = p.filter(|s| !s.trim().is_empty()); }
pub fn forget_ffmpeg_cache() { let _ = std::fs::remove_file(cache_file()); }
pub fn ffmpeg_works(p: &str) -> bool { usable(p) }
fn run_list(p: &str, what: &str) -> String { Command::new(p).args(["-hide_banner", what]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default() }
/// Why this ffmpeg can't be used for Marbeat exports (None = fine). Many bundled copies are stripped down.
pub fn ffmpeg_problem(p: &str) -> Option<String> {
    if !works(p) { return Some("doesn't run".into()); }
    let dem = run_list(p, "-demuxers");
    if !dem.lines().any(|l| l.contains(" rawvideo")) { return Some("can't read raw video (stripped-down build)".into()); }
    let mux = run_list(p, "-muxers");
    if !mux.lines().any(|l| l.split_whitespace().nth(1) == Some("mp4")) { return Some("can't write MP4".into()); }
    let enc = run_list(p, "-encoders");
    if !["libx264", "h264_nvenc", "libopenh264", "h264_mf", "h264_qsv", "h264_amf"].iter().any(|e| enc.contains(e)) { return Some("has no H.264 encoder".into()); }
    if !enc.contains("aac") { return Some("has no AAC encoder".into()); }
    None
}
fn usable(p: &str) -> bool { ffmpeg_problem(p).is_none() }
/// Looks for ffmpeg everywhere sensible on this machine and remembers what it found.
pub fn find_ffmpeg() -> Option<String> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(p) = OVERRIDE.lock().unwrap().clone() { if usable(&p) { return Some(p); } }
    if let Ok(t) = std::fs::read_to_string(cache_file()) { let t = t.trim(); if !t.is_empty() && std::path::Path::new(t).exists() && usable(t) { return Some(t.into()); } }
    let remember = |p: &str| { let f = cache_file(); if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); } let _ = std::fs::write(f, p); p.to_string() };
    if let Ok(p) = std::env::var("MARBEAT_FFMPEG") { if usable(&p) { return Some(remember(&p)); } }
    let mut quick: Vec<std::path::PathBuf> = vec![];
    if let Ok(p) = std::env::current_exe() { if let Some(d) = p.parent() { quick.push(d.join(exe)); quick.push(d.join("bin").join(exe)); } }
    if let Ok(d) = std::env::var("LOCALAPPDATA") { quick.push(std::path::Path::new(&d).join("Marbeat").join(exe)); }
    for p in quick { if p.exists() && usable(&p.to_string_lossy()) { return Some(remember(&p.to_string_lossy())); } }
    if usable(exe) { return Some(exe.into()); } // on PATH
    // common install spots, then a bounded scan of the machine
    let mut roots: Vec<std::path::PathBuf> = vec![];
    if cfg!(windows) {
        for v in ["ProgramFiles", "ProgramFiles(x86)", "ProgramData", "LOCALAPPDATA", "APPDATA", "USERPROFILE"] { if let Ok(d) = std::env::var(v) { roots.push(d.into()); } }
        for d in b'C'..=b'H' { roots.push(format!("{}:\\", d as char).into()); }
    } else {
        for d in ["/usr/local/bin", "/opt", "/snap", "/usr/bin"] { roots.push(d.into()); }
        if let Ok(h) = std::env::var("HOME") { roots.push(h.into()); }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
    for r in roots {
        let mut found = vec![];
        scan(&r, exe, if r.parent().is_none() { 4 } else { 6 }, deadline, &mut found);
        for p in found { let s = p.to_string_lossy().to_string(); if usable(&s) { return Some(remember(&s)); } }
    }
    None
}

fn write_wav(path: &str, song: &Song, secs: f64) -> std::io::Result<()> {
    let n = ((secs * song.sr as f64) as usize * 2).min(song.stereo.len());
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let (bytes, sr) = (n as u32 * 2, song.sr);
    f.write_all(b"RIFF")?; f.write_all(&(36 + bytes).to_le_bytes())?; f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?; f.write_all(&1u16.to_le_bytes())?; f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?; f.write_all(&(sr * 4).to_le_bytes())?; f.write_all(&4u16.to_le_bytes())?; f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?; f.write_all(&bytes.to_le_bytes())?;
    for &s in &song.stereo[..n] { f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?; }
    Ok(())
}

/// `scale` 2 = 1080x1920. Returns Err(message) on failure. `progress(frame,total)`; return false to cancel.
pub fn export_mp4(sc: &Scene, song: Option<&Song>, out: &str, scale: u32, mut progress: impl FnMut(usize, usize) -> bool) -> Result<(), String> {
    let ff = find_ffmpeg().ok_or("ffmpeg not found (put ffmpeg.exe next to Marbeat)")?;
    let (w, h) = (540 * scale, 960 * scale);
    let secs = sc.tl.win_time + 5.0;
    let total = (secs * FPS) as usize;
    let wav = format!("{}.tmp.wav", out);
    let mut cmd = Command::new(&ff);
    cmd.args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{h}"), "-r", &FPS.to_string(), "-i", "-"]);
    if let Some(s) = song { write_wav(&wav, s, secs).map_err(|e| e.to_string())?; cmd.args(["-i", &wav, "-af", "loudnorm=I=-14:TP=-1.5:LRA=11"]); }
    let encs = Command::new(&ff).args(["-hide_banner", "-encoders"]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let br = if scale >= 2 { "24M" } else { "9M" };
    if encs.contains("libx264") { cmd.args(["-c:v", "libx264", "-preset", "medium", "-crf", "19"]); }
    else if encs.contains("h264_nvenc") { cmd.args(["-c:v", "h264_nvenc", "-preset", "p5", "-cq", "21", "-b:v", br]); }
    else if encs.contains("libopenh264") { cmd.args(["-c:v", "libopenh264", "-b:v", br]); }
    else { cmd.args(["-c:v", "h264_mf", "-b:v", br, "-rate_control", "cbr"]); }
    cmd.args(["-pix_fmt", "yuv420p", "-ar", "48000", "-c:a", "aac", "-b:a", "256k", "-shortest", "-movflags", "+faststart", out]);
    let mut child = cmd.stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().unwrap();
    let mut r = Renderer::new(w, h);
    let mut ok = true;
    for f in 0..total {
        if !progress(f, total) { ok = false; break; }
        r.draw(sc, f as f64 / FPS);
        // tiny-skia is premultiplied; the frame is opaque so RGBA bytes are fine
        if stdin.write_all(r.pm.data()).is_err() { break; }
    }
    drop(stdin);
    let o = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&wav);
    if !ok { let _ = std::fs::remove_file(out); return Err("cancelled".into()); }
    if !o.status.success() { return Err(String::from_utf8_lossy(&o.stderr).to_string()); }
    Ok(())
}
