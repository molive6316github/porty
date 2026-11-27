//! One-click ffmpeg: download a full, pinned build, check its SHA-256, unpack just the ffmpeg binary next to Marbeat's settings.
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;

const URL: &str = "https://github.com/GyanD/codexffmpeg/releases/download/7.1.1/ffmpeg-7.1.1-essentials_build.zip";
const SHA256: &str = "04861d3339c5ebe38b56c19a15cf2c0cc97f5de4fa8910e4d47e5e6404e4a2d4";
pub const SIZE_LABEL: &str = "92 MB";

pub fn dest_dir() -> PathBuf {
    let b = std::env::var("LOCALAPPDATA").map(PathBuf::from).or_else(|_| std::env::var("HOME").map(|h| std::path::Path::new(&h).join(".config"))).unwrap_or_else(|_| ".".into());
    b.join("Marbeat")
}

pub fn get_ffmpeg(progress: impl Fn(u64, u64)) -> Result<PathBuf, String> {
    let (url, check) = match std::env::var("MARBEAT_FFMPEG_ZIP_URL") { Ok(u) => (u, false), Err(_) => (URL.to_string(), true) };
    let dir = dest_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let zip_path = dir.join("ffmpeg-download.zip");
    let resp = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(20)).build().get(&url).call().map_err(|e| format!("couldn't download ffmpeg: {e}"))?;
    let total: u64 = resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut r = resp.into_reader();
    let mut f = std::fs::File::create(&zip_path).map_err(|e| e.to_string())?;
    let (mut h, mut buf, mut got) = (Sha256::new(), vec![0u8; 1 << 17], 0u64);
    loop {
        let n = r.read(&mut buf).map_err(|e| format!("download interrupted: {e}"))?;
        if n == 0 { break; }
        h.update(&buf[..n]);
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        progress(got, total);
    }
    drop(f);
    let sum: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if check && sum != SHA256 { let _ = std::fs::remove_file(&zip_path); return Err("the download was corrupted (checksum mismatch). Try again.".into()); }
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let out = dir.join(exe);
    let res = (|| -> Result<(), String> {
        let mut z = zip::ZipArchive::new(std::fs::File::open(&zip_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        for i in 0..z.len() {
            let mut e = z.by_index(i).map_err(|e| e.to_string())?;
            let name = e.name().replace('\\', "/");
            if name == exe || name.ends_with(&format!("/bin/{exe}")) {
                let tmp = dir.join(format!("{exe}.part"));
                let mut o = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
                std::io::copy(&mut e, &mut o).map_err(|e| e.to_string())?;
                drop(o);
                let _ = std::fs::remove_file(&out);
                std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
                #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)); }
                return Ok(());
            }
        }
        Err("the download didn't contain ffmpeg".into())
    })();
    let _ = std::fs::remove_file(&zip_path);
    res?;
    Ok(out)
}
