//! Offline HD export: render every frame with the pro finish, pipe raw RGBA to ffmpeg, mux the song in.
use crate::{audio::Song, render::*, sim::FPS};
use std::io::Write;
use std::process::{Command, Stdio};

pub fn find_ffmpeg() -> Option<String> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut c = vec![];
    if let Ok(p) = std::env::current_exe() { if let Some(d) = p.parent() { c.push(d.join(exe)); } }
    if let Ok(d) = std::env::var("LOCALAPPDATA") { c.push(std::path::Path::new(&d).join("Marbeat").join(exe)); }
    for p in c { if p.exists() { return Some(p.to_string_lossy().into()); } }
    if Command::new(exe).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false) { return Some(exe.into()); }
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
