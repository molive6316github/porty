//! Self-update: checks a small JSON on the assets host, downloads the new exe, verifies its SHA-256, swaps itself in, relaunches.
use sha2::{Digest, Sha256};
use std::io::Read;

pub const VERSION: u32 = 22; // bump with every release; native.json "version" > this means an update exists
const DEFAULT_URL: &str = "https://assets.molive6316.com/marbeat/native.json";

#[derive(Clone, Debug)]
pub struct Info {
    pub version: u32,
    pub notes: String,
    pub url: String,
    pub sha256: String,
}

static URL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// a custom update source is only used if it really points at a .json manifest (an exe link pasted here would be read as the manifest)
pub fn valid_source(s: &str) -> bool { let t = s.trim().split(['?', '#']).next().unwrap_or("").to_lowercase(); t.starts_with("http") && t.ends_with(".json") }
pub fn set_url(u: Option<String>) { *URL.lock().unwrap() = u.filter(|s| valid_source(s)); }
pub fn default_url() -> &'static str { DEFAULT_URL }
fn manifest_url() -> String { std::env::var("MARBEAT_UPDATE_URL").ok().or_else(|| URL.lock().unwrap().clone()).unwrap_or_else(|| DEFAULT_URL.into()) }

/// a short human reason for a failed request (the user can read it in Settings and tell me)
pub fn explain(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("the server answered {code}"),
        ureq::Error::Transport(t) => {
            let m = t.to_string().to_lowercase();
            let why = if m.contains("certificate") || m.contains("tls") || m.contains("handshake") { "the network's security certificate wasn't trusted" }
                else if m.contains("dns") || m.contains("resolve") || m.contains("lookup") { "the address couldn't be looked up (DNS or offline)" }
                else if m.contains("timed out") || m.contains("timeout") { "the connection timed out" }
                else if m.contains("refused") || m.contains("reset") || m.contains("forbidden") || m.contains("403") { "the network blocked the connection" }
                else { "the connection failed" };
            format!("{why} ({t})")
        }
    }
}

pub fn check() -> Result<Option<Info>, String> {
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).build();
    let resp = agent.get(&manifest_url()).call().map_err(|e| explain(&e))?;
    let (ctype, clen, final_url) = (resp.header("Content-Type").unwrap_or("?").to_string(), resp.header("Content-Length").unwrap_or("?").to_string(), resp.get_url().to_string());
    // the manifest is tiny: read a little, and if it isn't the manifest say what came back instead
    let mut raw = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(resp.into_reader(), 64 * 1024), &mut raw).map_err(|e| e.to_string())?;
    let body = String::from_utf8_lossy(&raw).to_string();
    let j: serde_json::Value = match serde_json::from_str(&body) {
        Ok(j) => j,
        Err(_) => {
            let peek: String = body.chars().filter(|c| !c.is_control()).take(100).collect();
            return Err(format!("the server sent something that isn't the update file (type {ctype}, size {clen}, from {final_url}): \"{peek}\""));
        }
    };
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
    let resp = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(15)).build().get(&info.url).call().map_err(|e| explain(&e))?;
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
