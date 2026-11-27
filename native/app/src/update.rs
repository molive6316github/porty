//! Self-update: checks a small JSON on the assets host, downloads the new exe, verifies its SHA-256, swaps itself in, relaunches.
use sha2::{Digest, Sha256};
use std::io::Read;

pub const VERSION: u32 = 8; // bump with every release; native.json "version" > this means an update exists
const DEFAULT_URL: &str = "https://assets.molive6316.com/marbeat/native.json";

#[derive(Clone, Debug)]
pub struct Info {
    pub version: u32,
    pub notes: String,
    pub url: String,
    pub sha256: String,
}

static URL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
pub fn set_url(u: Option<String>) { *URL.lock().unwrap() = u.filter(|s| !s.trim().is_empty()); }
pub fn default_url() -> &'static str { DEFAULT_URL }
fn manifest_url() -> String { std::env::var("MARBEAT_UPDATE_URL").ok().or_else(|| URL.lock().unwrap().clone()).unwrap_or_else(|| DEFAULT_URL.into()) }

pub fn check() -> Result<Option<Info>, String> {
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).build();
    let body = agent.get(&manifest_url()).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    let j: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let version = j["version"].as_u64().ok_or("bad manifest")? as u32;
    if version <= VERSION { return Ok(None); }
    let base = manifest_url();
    let url = j["url"].as_str().ok_or("bad manifest")?;
    let url = if url.starts_with("http") { url.to_string() } else { format!("{}/{}", base.rsplit_once('/').map(|x| x.0).unwrap_or(""), url) };
    Ok(Some(Info { version, notes: j["notes"].as_str().unwrap_or("").into(), url, sha256: j["sha256"].as_str().unwrap_or("").to_lowercase() }))
}

/// Downloads, verifies, replaces the running exe and relaunches. Only returns on error.
pub fn install(info: &Info, progress: impl Fn(u64, u64)) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let new = exe.with_extension("new");
    let resp = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(15)).build().get(&info.url).call().map_err(|e| e.to_string())?;
    let total: u64 = resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut r = resp.into_reader();
    let mut f = std::fs::File::create(&new).map_err(|e| e.to_string())?;
    let (mut h, mut buf, mut got) = (Sha256::new(), vec![0u8; 1 << 16], 0u64);
    loop {
        let n = r.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        h.update(&buf[..n]);
        std::io::Write::write_all(&mut f, &buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        progress(got, total);
    }
    drop(f);
    let sum: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !info.sha256.is_empty() && sum != info.sha256 { let _ = std::fs::remove_file(&new); return Err("download was corrupted (checksum mismatch)".into()); }
    if got < 100_000 { let _ = std::fs::remove_file(&new); return Err("download too small".into()); }
    // a running exe can be renamed (not overwritten) on Windows; do the same on Linux for symmetry
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&new, &exe) { let _ = std::fs::rename(&old, &exe); return Err(e.to_string()); }
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)); }
    std::process::Command::new(&exe).spawn().map_err(|e| e.to_string())?;
    std::process::exit(0);
}

pub fn cleanup_old() { if let Ok(e) = std::env::current_exe() { let _ = std::fs::remove_file(e.with_extension("old")); } }
