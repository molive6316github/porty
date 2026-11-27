//! Projects and the story library: plain files under the Marbeat data folder.
//! Projects/<name>.marbeat holds one race (song, cast, director plan, settings, lyrics, attached story).
//! Stories/<name>.story holds a story script, editable inside the app.
use std::path::PathBuf;

pub fn base() -> PathBuf {
    let b = std::env::var("LOCALAPPDATA").map(PathBuf::from).or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".config"))).unwrap_or_else(|_| ".".into());
    b.join("Marbeat")
}
fn dir(sub: &str) -> PathBuf { let d = base().join(sub); let _ = std::fs::create_dir_all(&d); d }
pub fn projects_dir() -> PathBuf { dir("Projects") }
pub fn stories_dir() -> PathBuf { dir("Stories") }

/// a file name that is safe on Windows and keeps the name readable
pub fn safe(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_alphanumeric() || " -_()'!&".contains(c) { c } else { '_' }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "Untitled".into() } else { s.chars().take(60).collect() }
}

pub struct Entry { pub name: String, pub path: PathBuf, pub modified: std::time::SystemTime, pub info: String }

fn list(d: PathBuf, ext: &str, info: impl Fn(&str) -> String) -> Vec<Entry> {
    let mut v: Vec<Entry> = std::fs::read_dir(d).into_iter().flatten().flatten().filter_map(|e| {
        let p = e.path();
        if p.extension()?.to_str()? != ext { return None; }
        let modified = e.metadata().ok()?.modified().ok()?;
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        Some(Entry { name: p.file_stem()?.to_string_lossy().to_string(), info: info(&text), path: p, modified })
    }).collect();
    v.sort_by(|a, b| b.modified.cmp(&a.modified));
    v
}

pub fn list_projects() -> Vec<Entry> {
    list(projects_dir(), "marbeat", |t| {
        let j: serde_json::Value = serde_json::from_str(t).unwrap_or_default();
        let song = j["song_path"].as_str().filter(|s| !s.is_empty()).map(|s| std::path::Path::new(s).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()).unwrap_or_else(|| "no song".into());
        let n = j["n"].as_u64().unwrap_or(6);
        let story = if j["story"].as_str().map(|s| !s.trim().is_empty()).unwrap_or(false) { " · story" } else { "" };
        format!("{song} · {n} marbles{story}")
    })
}

pub fn list_stories() -> Vec<Entry> {
    list(stories_dir(), "story", |t| {
        let lines = t.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('@')).count();
        let races = t.lines().filter(|l| l.trim_start().starts_with("@race")).count();
        format!("{lines} line{} · {races} race{}", if lines == 1 { "" } else { "s" }, if races == 1 { "" } else { "s" })
    })
}

/// a free path for `name` in `d` (adds " 2", " 3"… if taken, unless `keep` is that file)
pub fn free_path(d: &std::path::Path, name: &str, ext: &str, keep: Option<&std::path::Path>) -> PathBuf {
    let base = safe(name);
    for k in 1..1000 {
        let n = if k == 1 { base.clone() } else { format!("{base} {k}") };
        let p = d.join(format!("{n}.{ext}"));
        if !p.exists() || keep == Some(p.as_path()) { return p; }
    }
    d.join(format!("{base} {}.{ext}", std::process::id()))
}

pub fn ago(t: std::time::SystemTime) -> String {
    let s = t.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    match s { 0..=59 => "just now".into(), 60..=3599 => format!("{} min ago", s / 60), 3600..=86399 => format!("{} h ago", s / 3600), _ => format!("{} days ago", s / 86400) }
}

pub const STORY_TEMPLATE: &str = "@char Ruby color=#ff5a5a blip=square pitch=520 speed=1.0
@char Azure color=#5ac8ff blip=triangle pitch=380 speed=0.9

* It was the night before the big race.
Ruby: You ready? {p} Because I am.
Azure: Ready to watch you lose.

@race map=neon seed=night marbles=Ruby,Azure winner=Ruby len=30

Ruby: Told you. {p} {winner} wins.
";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_safe() {
        assert_eq!(safe("  my/race: 2?  "), "my_race_ 2_");
        assert_eq!(safe(""), "Untitled");
        assert_eq!(safe("con."), "con_");
    }
}
