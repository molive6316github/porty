mod ai;
mod projects;
mod tools;
mod ui;
mod update;
use eframe::egui;
use marbeat_core::course::MAPS;
use marbeat_core::render::*;
use marbeat_core::sim::*;
use std::sync::{atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}, Arc, Mutex};
use ui::*;
use std::time::Instant;
use marbeat_core::audio::{load_song, Song};
use rodio::{buffer::SamplesBuffer, OutputStream, OutputStreamHandle, Sink, Source};
use marbeat_core::story;

const COLS: [[u8; 3]; 8] = [[255, 90, 90], [90, 200, 255], [120, 255, 140], [255, 210, 80], [200, 120, 255], [255, 150, 210], [255, 150, 60], [90, 255, 220]];
const NAMES: [&str; 8] = ["Ruby", "Azure", "Mint", "Sunny", "Violet", "Rosie", "Amber", "Teal"];

struct App {
    map: usize,
    seed: String,
    n: usize,
    len: f32,
    tl: Option<Timeline>,
    beats: Vec<f64>,
    chars: Vec<CharStyle>,
    r: Renderer,
    tex: Option<egui::TextureHandle>,
    t: f64,
    playing: bool,
    last: Instant,
    song: Option<Song>,
    status: String,
    _stream: Option<(OutputStream, OutputStreamHandle)>,
    sink: Option<Sink>,
    prog: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    exporting: Arc<AtomicBool>,
    result: Arc<Mutex<Option<String>>>,
    hd: bool,
    upd: Arc<Mutex<Option<update::Info>>>,
    upd_msg: Arc<Mutex<String>>,
    upd_busy: Arc<AtomicBool>,
    roster: Vec<([u8; 3], String)>, // (colour, name): editable, saved to disk
    rig: bool,
    plan: Vec<(f64, usize)>,
    rig_winner: Option<usize>,
    active: Vec<usize>,      // roster indices racing right now
    eff_seed: String,
    tour: Option<Tour>,
    tab: u8,
    spk: Vec<i64>,
    clones: Vec<Option<String>>,
    lyrics: String,
    voiced: Option<Vec<f32>>,
    ai_status: Arc<Mutex<String>>,
    ai_busy: Arc<AtomicBool>,
    ai_out: Arc<Mutex<Option<Result<ai::Result_, String>>>>,
    auto_update: bool,
    update_url: String,
    ffmpeg_path: String,           // manual override, empty = auto
    ff_found: Arc<Mutex<Option<String>>>,
    ff_busy: Arc<AtomicBool>,
    upd_checking: Arc<AtomicBool>,
    upd_got: Arc<AtomicU64>,
    upd_total: Arc<AtomicU64>,
    toast: Option<(String, Instant)>,
    confirm_del: bool,
    confirm_reset: bool,
    ff_dl: Arc<AtomicBool>,
    ff_got: Arc<AtomicU64>,
    ff_total: Arc<AtomicU64>,
    ff_msg: Arc<Mutex<String>>,
    singing: bool,
    crowd: bool,
    sync_style: u8, // 0 = track race, 1 = lane pads, 2 = lane pads 2 (race look)
    sync_music: bool,
    av_offset_ms: i32,
    show_lyrics: bool,
    song_path: String,
    lyric_lines: Vec<LyricLine>,
    lyr_draft: Option<Vec<Draft>>,
    lyr_paste: String,
    lyr_busy: Arc<AtomicBool>,
    lyr_status: Arc<Mutex<String>>,
    lyr_out: Arc<Mutex<Option<Result<Vec<marbeat_ai::sing::Line>, String>>>>,
    cur_plan: Vec<(f64, usize)>,
    cur_winner: Option<usize>,
    cur_len: f64,
    cur_bars: Vec<(f64, f32)>,
    cur_compose: bool,
    story: Option<StoryRun>,
    story_name: String,
    race_title: String,
    audio_t0: f64,                 // race time the current audio sink started at (the clock follows the audio)
    proj_path: Option<std::path::PathBuf>,
    proj_name: String,
    proj_dirty: bool,
    proj_story: String,            // story script attached to this project
    proj_rename: Option<(std::path::PathBuf, String)>,
    proj_del: Option<std::path::PathBuf>,
    new_name: String,
    st_edit: Option<StoryEdit>,
    st_del: Option<std::path::PathBuf>,
    lists: Option<(Vec<projects::Entry>, Vec<projects::Entry>, Instant)>,
    last_export: Option<String>,
    export_start: Option<Instant>,
    recent: Vec<String>,
    show_keys: bool,
}

/// a story script open in the editor
struct StoryEdit { path: Option<std::path::PathBuf>, name: String, text: String, saved: String }

/// A lyric line being checked by the human before it goes on screen.
#[derive(Clone)]
struct Draft { t0: f64, t1: f64, text: String, orig: Vec<(String, f64)>, orig_text: String }

struct StoryRun { player: story::Player, saved_roster: Vec<([u8; 3], String)>, saved_n: usize, saved_rig: (bool, Option<usize>), blip_n: u32 }

#[derive(Default)]
struct Acts { confirm_lyrics: bool, rebuild: bool, load_path: Option<String>, toggled: bool, export: bool, save: bool, tour_next: bool, sing_go: bool, restart_audio: bool, seek: Option<f64> }

#[derive(Clone, Default)]
struct Tour { remaining: Vec<usize>, next: Vec<usize>, heat: usize, round: usize, log: Vec<String>, champion: Option<usize>, recorded: bool }

fn cfg_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").map(std::path::PathBuf::from).or_else(|_| std::env::var("HOME").map(|h| std::path::Path::new(&h).join(".config"))).unwrap_or_else(|_| ".".into());
    let d = base.join("Marbeat");
    let _ = std::fs::create_dir_all(&d);
    d.join("settings.json")
}

impl App {
    fn new() -> Self {
        let mut a = App { map: 0, seed: "42".into(), n: 6, len: 40.0, tl: None, beats: (0..2000).map(|i| i as f64 * 0.5).collect(), chars: vec![], r: Renderer::new(405, 720), tex: None, t: 0.0, playing: true, last: Instant::now(), song: None, status: "No song loaded".into(), _stream: OutputStream::try_default().ok(), sink: None, prog: Default::default(), total: Default::default(), cancel: Default::default(), exporting: Default::default(), result: Default::default(), hd: true, upd: Default::default(), upd_msg: Default::default(), upd_busy: Default::default(),
            roster: (0..8).map(|i| (COLS[i], NAMES[i].to_string())).collect(), rig: false, plan: vec![], rig_winner: None, active: vec![], eff_seed: String::new(), tour: None, tab: 0, spk: vec![10, 25, 40, 60, 75, 90, 5, 100], clones: vec![None; 8], lyrics: String::new(), voiced: None, ai_status: Default::default(), ai_busy: Default::default(), ai_out: Default::default(), auto_update: true, update_url: String::new(), ffmpeg_path: String::new(), ff_found: Default::default(), ff_busy: Default::default(), upd_checking: Default::default(), upd_got: Default::default(), upd_total: Default::default(), toast: None, confirm_del: false, confirm_reset: false, ff_dl: Default::default(), ff_got: Default::default(), ff_total: Default::default(), ff_msg: Default::default(), singing: false, crowd: false, sync_style: 0, sync_music: true, av_offset_ms: 0, show_lyrics: true, song_path: String::new(), lyric_lines: vec![], lyr_draft: None, lyr_paste: String::new(), lyr_busy: Default::default(), lyr_status: Default::default(), lyr_out: Default::default(), cur_plan: vec![], cur_winner: None, cur_len: 40.0, cur_bars: vec![], cur_compose: false, story: None, story_name: String::new(), race_title: "Marbeat".into(), audio_t0: 0.0, proj_path: None, proj_name: String::new(), proj_dirty: false, proj_story: String::new(), proj_rename: None, proj_del: None, new_name: String::new(), st_edit: None, st_del: None, lists: None, last_export: None, export_start: None, recent: vec![], show_keys: false };
        a.load_cfg();
        update::set_url(Some(a.update_url.clone()));
        marbeat_core::export::set_ffmpeg_override(Some(a.ffmpeg_path.clone()));
        if a.auto_update { a.check_updates(); }
        if let Ok(sp) = std::env::var("MARBEAT_SONG") { a.load(&sp); if std::env::var("MARBEAT_DETECT").is_ok() { a.start_detect(None); } }
        if std::env::var("MARBEAT_STORY").is_ok() { let src = include_str!("../../assets/demo.story"); a.start_story(src, "Demo"); }
        if std::env::var("MARBEAT_SONG").is_err() { if let Some(p) = a.proj_path.clone() { a.open_project(&p); } }
        a.detect_ffmpeg();
        if let Ok(t) = std::env::var("MARBEAT_TAB") { a.tab = t.parse().unwrap_or(0); }
        update::cleanup_old();
        a.rebuild();
        a
    }
    fn start_audio(&mut self) {
        self.sink = None;
        if let (Some(song), Some((_, h))) = (&self.song, &self._stream) {
            let stereo: &Vec<f32> = self.voiced.as_ref().unwrap_or(&song.stereo);
            if let Ok(sink) = Sink::try_new(h) {
                let skip = (self.t * song.sr as f64) as usize * 2;
                if skip < stereo.len() {
                    self.audio_t0 = self.t;
                    sink.append(SamplesBuffer::new(2, song.sr, stereo[skip..].to_vec()));
                    if !self.playing { sink.pause(); }
                    self.sink = Some(sink);
                }
            }
        }
    }
    fn load(&mut self, path: &str) {
        match load_song(path) {
            Ok(s) => {
                self.status = format!("{} · {:.0} BPM · {:.0}s", std::path::Path::new(path).file_name().unwrap().to_string_lossy(), s.bpm, s.mono.len() as f64 / s.sr as f64);
                self.len = (s.mono.len() as f64 / s.sr as f64).min(300.0) as f32;
                self.beats = s.beats.clone();
                self.song = Some(s);
                self.song_path = path.to_string();
                self.recent.retain(|r| r != path); self.recent.insert(0, path.to_string()); self.recent.truncate(6); self.save_cfg();
                self.lyr_draft = None;
                self.load_saved_lyrics();
                self.rebuild();
            }
            Err(e) => self.status = format!("Couldn't load: {e}"),
        }
    }
    fn export(&mut self) {
        let fname = format!("{}.mp4", projects::safe(if self.proj_name.trim().is_empty() { "marbeat" } else { &self.proj_name }));
        let mut dlg = rfd::FileDialog::new().add_filter("MP4 video", &["mp4"]).set_file_name(&fname);
        if let Some(d) = self.last_export.as_ref().and_then(|p| std::path::Path::new(p).parent().map(|d| d.to_path_buf())) { dlg = dlg.set_directory(d); }
        let Some(out) = dlg.save_file() else { return };
        self.last_export = Some(out.to_string_lossy().to_string());
        self.export_start = Some(Instant::now());
        let out = out.to_string_lossy().to_string();
        let (key, seed, n, len, beats, hd) = (MAPS[self.map].key, self.eff_seed.clone(), self.active.len(), self.cur_len, self.beats.clone(), self.hd);
        let plan = self.cur_plan.clone();
        let rw = self.cur_winner;
        let lyr = if self.show_lyrics { self.lyric_lines.clone() } else { vec![] };
        let (bars_s, compose) = (self.cur_bars.clone(), self.cur_compose);
        let crowd_n = if self.crowd { 3 } else { 0 };
        let sync_style = self.sync_style;
        let song = self.song.as_ref().map(|s| Song { sr: s.sr, mono: vec![], stereo: self.voiced.clone().unwrap_or_else(|| s.stereo.clone()), beats: vec![], bpm: s.bpm });
        let chars: Vec<(String, [u8; 3])> = self.chars.iter().map(|c| (c.name.clone(), c.color)).collect();
        let title_s = self.race_title.clone();
        let (prog, total, cancel, busy, res) = (self.prog.clone(), self.total.clone(), self.cancel.clone(), self.exporting.clone(), self.result.clone());
        cancel.store(false, Ordering::SeqCst);
        busy.store(true, Ordering::SeqCst);
        prog.store(0, Ordering::SeqCst);
        std::thread::spawn(move || {
            let tl = build_timeline(&RaceSetup { seed: &seed, map: key, n, len, beats: &beats, plan: &plan, winner: rw, bars: &bars_s, compose: compose && sync_style >= 1, crowd: crowd_n, designed: compose && sync_style == 0, race: sync_style == 2 });
            let cs: Vec<CharStyle> = chars.iter().map(|(n, c)| CharStyle { name: n.clone(), color: *c, label: "Marble".into() }).collect();
            let sc = Scene { lyrics: &lyr, bars: &bars_s, tl: &tl, chars: &cs, beats: &beats, title: &title_s, pro_post: true };
            let r = marbeat_core::export::export_mp4(&sc, song.as_ref(), &out, if hd { 2 } else { 1 }, |f, t| { prog.store(f, Ordering::Relaxed); total.store(t, Ordering::Relaxed); !cancel.load(Ordering::Relaxed) });
            *res.lock().unwrap() = Some(match r { Ok(()) => format!("Saved {out}"), Err(e) => format!("Export failed: {}", e.lines().next().unwrap_or("")) });
            busy.store(false, Ordering::SeqCst);
        });
    }
    fn load_cfg(&mut self) {
        if let Ok(t) = std::fs::read_to_string(cfg_path()) {
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&t) {
                if let Some(a) = j["roster"].as_array() {
                    for (i, r) in a.iter().enumerate().take(8) {
                        if let (Some(n), Some(c)) = (r["name"].as_str(), r["color"].as_array()) {
                            if c.len() == 3 { self.roster[i] = ([c[0].as_u64().unwrap_or(255) as u8, c[1].as_u64().unwrap_or(255) as u8, c[2].as_u64().unwrap_or(255) as u8], n.to_string()); }
                        }
                    }
                }
                if let Some(a) = j["speakers"].as_array() { for (i, v) in a.iter().enumerate().take(8) { if let Some(x) = v.as_i64() { self.spk[i] = x; } } }
                if let Some(a) = j["clones"].as_array() { for (i, v) in a.iter().enumerate().take(8) { self.clones[i] = v.as_str().map(|x| x.to_string()); } }
                if let Some(x) = j["lyrics"].as_str() { self.lyrics = x.into(); }
                if let Some(x) = j["auto_update"].as_bool() { self.auto_update = x; }
                if let Some(x) = j["update_url"].as_str() { self.update_url = if update::valid_source(x) { x.into() } else { String::new() }; }
                if let Some(x) = j["ffmpeg_path"].as_str() { self.ffmpeg_path = x.into(); }
                if let Some(x) = j["singing"].as_bool() { self.singing = x; }
                if let Some(x) = j["sync_music"].as_bool() { self.sync_music = x; }
                if let Some(x) = j["crowd"].as_bool() { self.crowd = x; }
                if let Some(x) = j["sync_style"].as_u64() { if j["v22_track"].as_bool().unwrap_or(false) { self.sync_style = (x as u8).min(2); } }
                if let Some(r) = j["recent"].as_array() { self.recent = r.iter().filter_map(|x| x.as_str().map(String::from)).collect(); }
                if let Some(x) = j["last_export"].as_str() { self.last_export = Some(x.into()); }
                if let Some(x) = j["last_project"].as_str() { let p = std::path::PathBuf::from(x); if p.exists() { self.proj_path = Some(p); } }
                if let Some(x) = j["show_lyrics"].as_bool() { self.show_lyrics = x; }
                if let Some(x) = j["av_offset_ms"].as_i64() { self.av_offset_ms = x as i32; }
                if let Some(x) = j["hd"].as_bool() { self.hd = x; }
                if let Some(x) = j["seed"].as_str() { self.seed = x.into(); }
                if let Some(x) = j["n"].as_u64() { self.n = (x as usize).clamp(2, 8); }
                if let Some(x) = j["map"].as_u64() { self.map = (x as usize).min(MAPS.len() - 1); }
            }
        }
        self.rebuild();
    }
    fn save_cfg(&self) {
        if self.story.is_some() { return; }
        let j = serde_json::json!({"roster": self.roster.iter().map(|(c, n)| serde_json::json!({"name": n, "color": c})).collect::<Vec<_>>(), "auto_update": self.auto_update, "update_url": self.update_url, "ffmpeg_path": self.ffmpeg_path, "hd": self.hd, "singing": self.singing, "sync_music": self.sync_music, "crowd": self.crowd, "sync_style": self.sync_style, "show_lyrics": self.show_lyrics, "av_offset_ms": self.av_offset_ms, "speakers": self.spk, "clones": self.clones, "lyrics": self.lyrics, "seed": self.seed, "n": self.n, "map": self.map, "v22_track": true, "recent": self.recent, "last_export": self.last_export, "last_project": self.proj_path.as_ref().map(|p| p.to_string_lossy().to_string())});
        let _ = std::fs::write(cfg_path(), j.to_string());
    }
    fn check_updates(&self) {
        let (u, m, c) = (self.upd.clone(), self.upd_msg.clone(), self.upd_checking.clone());
        c.store(true, Ordering::SeqCst);
        *m.lock().unwrap() = "Checking…".into();
        std::thread::spawn(move || {
            match update::check() {
                Ok(Some(i)) => { *m.lock().unwrap() = format!("Version {} is available", i.version); *u.lock().unwrap() = Some(i); }
                Ok(None) => { *u.lock().unwrap() = None; *m.lock().unwrap() = format!("You're up to date (v{})", update::VERSION); }
                Err(e) => *m.lock().unwrap() = format!("Couldn't check for updates: {e}"),
            }
            c.store(false, Ordering::SeqCst);
        });
    }
    fn install_update(&self) {
        let Some(info) = self.upd.lock().unwrap().clone() else { return };
        let (m, b, g, t) = (self.upd_msg.clone(), self.upd_busy.clone(), self.upd_got.clone(), self.upd_total.clone());
        b.store(true, Ordering::SeqCst);
        std::thread::spawn(move || {
            *m.lock().unwrap() = "Downloading…".into();
            if let Err(e) = update::install(&info, |got, total| { g.store(got, Ordering::Relaxed); t.store(total, Ordering::Relaxed); }) { *m.lock().unwrap() = format!("Update failed: {e}"); }
            b.store(false, Ordering::SeqCst);
        });
    }
    fn detect_ffmpeg(&self) {
        let (f, b) = (self.ff_found.clone(), self.ff_busy.clone());
        b.store(true, Ordering::SeqCst);
        std::thread::spawn(move || { *f.lock().unwrap() = marbeat_core::export::find_ffmpeg(); b.store(false, Ordering::SeqCst); });
    }
    fn download_ffmpeg(&self) {
        let (b, g, t, m, f) = (self.ff_dl.clone(), self.ff_got.clone(), self.ff_total.clone(), self.ff_msg.clone(), self.ff_found.clone());
        let fb = self.ff_busy.clone();
        b.store(true, Ordering::SeqCst);
        *m.lock().unwrap() = "Downloading ffmpeg…".into();
        std::thread::spawn(move || {
            match tools::get_ffmpeg(|got, total| { g.store(got, Ordering::Relaxed); t.store(total, Ordering::Relaxed); }) {
                Ok(p) => {
                    marbeat_core::export::forget_ffmpeg_cache();
                    let ps = p.to_string_lossy().to_string();
                    match marbeat_core::export::ffmpeg_problem(&ps) {
                        None => { marbeat_core::export::set_ffmpeg_override(Some(ps.clone())); *f.lock().unwrap() = Some(ps); *m.lock().unwrap() = "ffmpeg is installed and ready.".into(); }
                        Some(why) => *m.lock().unwrap() = format!("Downloaded, but it {why}."),
                    }
                }
                Err(e) => *m.lock().unwrap() = format!("Couldn't get ffmpeg: {e}"),
            }
            let _ = &fb;
            b.store(false, Ordering::SeqCst);
        });
    }
    fn say(&mut self, s: impl Into<String>) { self.toast = Some((s.into(), Instant::now())); }
    fn play_blip(&mut self, idx: usize) {
        let Some(run) = &mut self.story else { return };
        run.blip_n = run.blip_n.wrapping_add(1);
        let (kind, pitch) = run.player.script.chars.get(idx).map(|c| (c.blip.clone(), c.pitch)).unwrap_or(("sine".into(), 280.0));
        let samples = story::blip(&kind, pitch, run.blip_n);
        if let Some((_, h)) = &self._stream { let _ = h.play_raw(SamplesBuffer::new(1, 22050, samples).convert_samples()); }
    }
    fn story_events(&mut self, evs: Vec<story::Ev>) {
        for e in evs {
            match e {
                story::Ev::Blip(i) => self.play_blip(i),
                story::Ev::StartRace(cfg) => self.story_race(cfg),
                story::Ev::Finished => self.stop_story(),
            }
        }
    }
    fn start_story(&mut self, src: &str, name: &str) {
        let script = story::parse(src);
        if script.beats.is_empty() { self.say("That script has no lines."); return; }
        if let Some(e) = script.errors.first() { self.say(format!("Script problem: {e}")); }
        self.stop_story();
        let saved_roster = self.roster.clone();
        for (i, c) in script.chars.iter().filter(|c| c.name != "Narrator").take(8).enumerate() { self.roster[i] = (c.color, c.name.clone()); }
        let saved = (saved_roster, self.n, (self.rig, self.rig_winner));
        self.n = script.chars.iter().filter(|c| c.name != "Narrator").count().clamp(2, 8);
        self.tour = None; self.voiced = None; self.sink = None; self.rig = false; self.rig_winner = None;
        self.story_name = name.to_string();
        self.race_title = String::new();
        self.rebuild();
        self.playing = false;
        let (player, evs) = story::Player::new(script);
        self.story = Some(StoryRun { player, saved_roster: saved.0, saved_n: saved.1, saved_rig: saved.2, blip_n: 0 });
        self.tab = 5;
        self.story_events(evs);
    }
    fn stop_story(&mut self) {
        if let Some(run) = self.story.take() {
            self.roster = run.saved_roster; self.n = run.saved_n; self.rig = run.saved_rig.0; self.rig_winner = run.saved_rig.1;
            self.race_title = "Marbeat".into(); self.sink = None; self.playing = false;
            self.rebuild();
            self.playing = false;
        }
    }
    fn story_race(&mut self, cfg: story::RaceCfg) {
        // put the named marbles first in the cast so they are the ones racing
        let names: Vec<String> = cfg.marbles.clone();
        if !names.is_empty() {
            let mut order: Vec<usize> = vec![];
            for n in &names { if let Some(i) = self.roster.iter().position(|r| &r.1 == n) { if !order.contains(&i) { order.push(i); } } }
            let rest: Vec<usize> = (0..self.roster.len()).filter(|i| !order.contains(i)).collect();
            let new: Vec<([u8; 3], String)> = order.iter().chain(rest.iter()).map(|&i| self.roster[i].clone()).collect();
            let sp: Vec<i64> = order.iter().chain(rest.iter()).map(|&i| self.spk[i]).collect();
            self.roster = new; self.spk = sp;
            self.n = order.len().clamp(2, 8);
        }
        if let Some(m) = &cfg.map { if let Some(i) = MAPS.iter().position(|d| d.key == m) { self.map = i; } }
        if let Some(sd) = &cfg.seed { self.seed = sd.clone(); }
        if let Some(l) = cfg.len { self.len = l.clamp(10.0, 180.0); }
        self.rig = cfg.winner.is_some();
        self.rig_winner = cfg.winner.as_ref().and_then(|w| self.roster.iter().take(self.n).position(|r| &r.1 == w));
        self.plan.clear();
        self.race_title = cfg.title.clone().unwrap_or_else(|| "Marbeat".into());
        self.sink = None;
        self.rebuild();
        self.playing = true;
        if self.song.is_some() { self.start_audio(); }
    }
    // ---------------- projects
    fn project_json(&self) -> serde_json::Value {
        serde_json::json!({
            "format": 1, "name": self.proj_name, "song_path": self.song_path, "seed": self.seed, "n": self.n, "map": MAPS[self.map].key, "len": self.len,
            "roster": self.roster.iter().map(|(c, n)| serde_json::json!({"name": n, "color": c})).collect::<Vec<_>>(),
            "speakers": self.spk, "clones": self.clones, "singing": self.singing, "lyrics": self.lyrics,
            "rig": self.rig, "rig_winner": self.rig_winner, "plan": self.plan,
            "sync_music": self.sync_music, "sync_style": self.sync_style, "crowd": self.crowd, "show_lyrics": self.show_lyrics, "hd": self.hd,
            "lyric_lines": self.lyric_lines.iter().map(|l| serde_json::json!({"t0": l.t0, "t1": l.t1, "words": l.words})).collect::<Vec<_>>(),
            "story": self.proj_story,
        })
    }
    fn save_project(&mut self) {
        if self.story.is_some() { self.say("Stop the story first, then save."); return; }
        if self.proj_name.trim().is_empty() {
            self.proj_name = if self.song_path.is_empty() { "My race".into() } else { std::path::Path::new(&self.song_path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("My race".into()) };
        }
        let d = projects::projects_dir();
        let path = match &self.proj_path { Some(p) => p.clone(), None => projects::free_path(&d, &self.proj_name, "marbeat", None) };
        self.proj_name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        match std::fs::write(&path, serde_json::to_string_pretty(&self.project_json()).unwrap()) {
            Ok(_) => { self.proj_path = Some(path); self.proj_dirty = false; self.lists = None; self.save_cfg(); }
            Err(e) => self.say(format!("Couldn't save the project: {e}")),
        }
    }
    fn open_project(&mut self, path: &std::path::Path) {
        let j: serde_json::Value = match std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()) { Some(j) => j, None => { self.say("That project file couldn't be read."); return; } };
        self.stop_story(); self.tour = None; self.voiced = None; self.sink = None;
        if let Some(r) = j["roster"].as_array() { for (i, v) in r.iter().enumerate().take(8) { if let (Some(n), Some(c)) = (v["name"].as_str(), v["color"].as_array()) { self.roster[i] = ([c.first().and_then(|x| x.as_u64()).unwrap_or(255) as u8, c.get(1).and_then(|x| x.as_u64()).unwrap_or(255) as u8, c.get(2).and_then(|x| x.as_u64()).unwrap_or(255) as u8], n.to_string()); } } }
        if let Some(a) = j["speakers"].as_array() { for (i, v) in a.iter().enumerate().take(8) { if let Some(x) = v.as_i64() { self.spk[i] = x; } } }
        if let Some(a) = j["clones"].as_array() { for (i, v) in a.iter().enumerate().take(8) { self.clones[i] = v.as_str().map(|x| x.to_string()); } }
        self.seed = j["seed"].as_str().unwrap_or("42").into();
        self.n = (j["n"].as_u64().unwrap_or(6) as usize).clamp(2, 8);
        self.map = j["map"].as_str().and_then(|k| MAPS.iter().position(|m| m.key == k)).unwrap_or(0);
        self.singing = j["singing"].as_bool().unwrap_or(false);
        self.lyrics = j["lyrics"].as_str().unwrap_or("").into();
        self.rig = j["rig"].as_bool().unwrap_or(false);
        self.rig_winner = j["rig_winner"].as_u64().map(|x| x as usize);
        self.plan = j["plan"].as_array().map(|a| a.iter().filter_map(|p| Some((p[0].as_f64()?, p[1].as_u64()? as usize))).collect()).unwrap_or_default();
        self.sync_music = j["sync_music"].as_bool().unwrap_or(true);
        self.sync_style = (j["sync_style"].as_u64().unwrap_or(0) as u8).min(2);
        self.crowd = j["crowd"].as_bool().unwrap_or(false);
        self.show_lyrics = j["show_lyrics"].as_bool().unwrap_or(true);
        if let Some(h) = j["hd"].as_bool() { self.hd = h; }
        self.proj_story = j["story"].as_str().unwrap_or("").into();
        self.proj_name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        self.proj_path = Some(path.to_path_buf());
        let song = j["song_path"].as_str().unwrap_or("").to_string();
        if !song.is_empty() && std::path::Path::new(&song).exists() { self.load(&song); }
        else {
            self.song = None; self.song_path = String::new(); self.beats = (0..2000).map(|i| i as f64 * 0.5).collect();
            self.status = if song.is_empty() { "No song loaded".into() } else { format!("Song missing: {song}") };
            if !song.is_empty() { self.say("This project's song file has moved. Open it again from Studio."); }
        }
        if let Some(len) = j["len"].as_f64() { if self.song.is_none() { self.len = len as f32; } }
        // lyrics travel with the project, so it still works if the lyric store was cleared
        if let Some(ls) = j["lyric_lines"].as_array() { if !ls.is_empty() {
            self.lyric_lines = ls.iter().map(|l| LyricLine { t0: l["t0"].as_f64().unwrap_or(0.0), t1: l["t1"].as_f64().unwrap_or(0.0), words: l["words"].as_array().map(|w| w.iter().map(|p| (p[0].as_str().unwrap_or("").to_string(), p[1].as_f64().unwrap_or(0.0))).collect()).unwrap_or_default() }).collect();
        } }
        self.t = 0.0; self.playing = false;
        self.rebuild();
        self.proj_dirty = false;
        self.save_cfg();
    }
    fn new_project(&mut self, name: &str) {
        self.stop_story(); self.tour = None; self.voiced = None; self.sink = None;
        self.song = None; self.song_path.clear(); self.status = "No song loaded".into(); self.beats = (0..2000).map(|i| i as f64 * 0.5).collect();
        self.lyric_lines.clear(); self.lyrics.clear(); self.plan.clear(); self.rig = false; self.rig_winner = None; self.proj_story.clear();
        self.seed = rand_seed(); self.len = 40.0; self.t = 0.0;
        self.proj_path = None; self.proj_name = if name.trim().is_empty() { "New race".into() } else { name.trim().to_string() };
        self.rebuild();
        self.save_project();
        self.tab = 0;
    }
    fn refresh_lists(&mut self) -> bool {
        let stale = self.lists.as_ref().map(|l| l.2.elapsed().as_secs() > 3).unwrap_or(true);
        if stale { self.lists = Some((projects::list_projects(), projects::list_stories(), Instant::now())); }
        true
    }
    fn projects_left(&mut self, ui: &mut egui::Ui, a: &mut Acts) {
        let _ = a;
        self.refresh_lists();
        card(ui, "This project", |ui| {
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.proj_name).hint_text("Name this project").desired_width(ui.available_width() - 70.0));
                if r.lost_focus() && self.proj_path.is_some() {
                    // rename the file to match
                    let old = self.proj_path.clone().unwrap();
                    let np = projects::free_path(&projects::projects_dir(), &self.proj_name, "marbeat", Some(&old));
                    if np != old && std::fs::rename(&old, &np).is_ok() { self.proj_path = Some(np); }
                    self.save_project();
                }
                if primary_w(ui, "Save", 60.0).clicked() { self.save_project(); self.say("Saved"); }
            });
            match &self.proj_path {
                Some(_) => muted(ui, if self.proj_dirty { "Unsaved changes. Ctrl+S saves." } else { "Saved. Changes save automatically." }),
                None => muted(ui, "Not saved yet. Save to keep this race, its cast, lyrics and story."),
            }
            if self.proj_path.is_some() && ghost(ui, "Save a copy").clicked() {
                self.proj_path = None; self.proj_name = format!("{} copy", self.proj_name); self.save_project(); self.say("Copy saved");
            }
        });
        card(ui, "New project", |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text("Name").desired_width(ui.available_width() - 76.0));
                if primary_w(ui, "Create", 66.0).clicked() { if self.proj_dirty && self.proj_path.is_some() { self.save_project(); } let n = std::mem::take(&mut self.new_name); self.new_project(&n); }
            });
            muted(ui, "Starts fresh: no song, new seed. Your cast and settings carry over.");
        });
        let projs = self.lists.as_ref().map(|l| l.0.iter().map(|e| (e.name.clone(), e.path.clone(), e.info.clone(), projects::ago(e.modified))).collect::<Vec<_>>()).unwrap_or_default();
        card(ui, &format!("All projects ({})", projs.len()), |ui| {
            if projs.is_empty() { muted(ui, "Nothing saved yet."); }
            for (name, path, info, when) in projs {
                let current = self.proj_path.as_ref() == Some(&path);
                egui::Frame::none().fill(if current { ACCENT.gamma_multiply(0.16) } else { CARD2 }).rounding(10.0).stroke(egui::Stroke::new(1.0, if current { ACCENT } else { EDGE })).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some((rp, rn)) = &mut self.proj_rename { if *rp == path {
                        let r = ui.add(egui::TextEdit::singleline(rn).desired_width(ui.available_width()));
                        if r.lost_focus() {
                            let np = projects::free_path(&projects::projects_dir(), rn, "marbeat", Some(&path));
                            if std::fs::rename(&path, &np).is_ok() && current { self.proj_path = Some(np.clone()); self.proj_name = np.file_stem().unwrap().to_string_lossy().to_string(); }
                            self.proj_rename = None; self.lists = None;
                        }
                        return;
                    } }
                    ui.label(egui::RichText::new(&name).strong());
                    muted(ui, format!("{info} · {when}"));
                    ui.horizontal_wrapped(|ui| {
                        if !current && ui.small_button("Open").clicked() { if self.proj_dirty && self.proj_path.is_some() { self.save_project(); } self.open_project(&path); }
                        if current { badge(ui, "open", MINT); }
                        if ui.small_button("Rename").clicked() { self.proj_rename = Some((path.clone(), name.clone())); }
                        if ui.small_button("Duplicate").clicked() { let np = projects::free_path(&projects::projects_dir(), &format!("{name} copy"), "marbeat", None); let _ = std::fs::copy(&path, &np); self.lists = None; }
                        if self.proj_del.as_ref() == Some(&path) {
                            if ui.add(egui::Button::new(egui::RichText::new("Really delete").color(egui::Color32::WHITE)).fill(BAD).small()).clicked() {
                                let _ = std::fs::remove_file(&path); if current { self.proj_path = None; self.proj_dirty = true; } self.proj_del = None; self.lists = None;
                            }
                        } else if ui.small_button("Delete").clicked() { self.proj_del = Some(path.clone()); }
                    });
                });
            }
            if ghost(ui, "Show the folder").clicked() { let _ = open_folder(&projects::projects_dir()); }
        });
    }

    // ---------------- stories
    fn save_story_edit(&mut self) {
        let Some(ed) = &mut self.st_edit else { return };
        let d = projects::stories_dir();
        let np = projects::free_path(&d, &ed.name, "story", ed.path.as_deref());
        if let Some(old) = &ed.path { if *old != np { let _ = std::fs::rename(old, &np); } }
        match std::fs::write(&np, &ed.text) {
            Ok(_) => { ed.name = np.file_stem().unwrap().to_string_lossy().to_string(); ed.path = Some(np); ed.saved = ed.text.clone(); self.lists = None; self.say("Story saved"); }
            Err(e) => self.say(format!("Couldn't save the story: {e}")),
        }
    }
    fn story_left(&mut self, ui: &mut egui::Ui, a: &mut Acts) {
        let _ = a;
        self.refresh_lists();
        if let Some(run) = &self.story {
            let (i, n) = run.player.progress();
            card(ui, "Now playing", |ui| {
                ui.label(egui::RichText::new(self.story_name.clone()).strong());
                ui.add(egui::ProgressBar::new(i as f32 / n.max(1) as f32).text(format!("{} / {}", i, n)).fill(ACCENT));
                muted(ui, "Space, Enter or a click moves on. A race can't be skipped.");
            });
            if ghost(ui, "Stop story").clicked() { self.stop_story(); }
            return;
        }
        // ---- editor
        if self.st_edit.is_some() {
            let mut close = false; let mut play = false; let mut save = false; let mut attach = false;
            card(ui, "Edit story", |ui| {
                let ed = self.st_edit.as_mut().unwrap();
                ui.add(egui::TextEdit::singleline(&mut ed.name).hint_text("Story name").desired_width(ui.available_width()));
                let parsed = story::parse(&ed.text);
                let chars = parsed.chars.iter().filter(|c| c.name != "Narrator").count();
                let races = ed.text.lines().filter(|l| l.trim_start().starts_with("@race")).count();
                muted(ui, format!("{} lines · {} characters · {} race{}{}", parsed.beats.len(), chars, races, if races == 1 { "" } else { "s" }, if ed.text != ed.saved { " · unsaved" } else { "" }));
                if let Some(e) = parsed.errors.first() { ui.label(egui::RichText::new(format!("⚠ {e}")).color(WARN).size(12.5)); }
                ui.horizontal_wrapped(|ui| {
                    if primary_w(ui, "Save", 64.0).clicked() { save = true; }
                    if ghost(ui, "▶ Play").clicked() { play = true; }
                    if ghost(ui, "Attach to project").on_hover_text("Saves this script inside the current project").clicked() { attach = true; }
                    if ghost(ui, "Close").clicked() { close = true; }
                });
            });
            card(ui, "Script", |ui| {
                let ed = self.st_edit.as_mut().unwrap();
                egui::ScrollArea::vertical().max_height(420.0).id_salt("st_scroll").show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut ed.text).code_editor().desired_rows(22).desired_width(f32::INFINITY));
                });
                if ui.small_button("+ character").clicked() { ed.text.push_str("\n@char Name color=#ffffff blip=sine pitch=440 speed=1.0\n"); }
                if ui.small_button("+ race").clicked() { ed.text.push_str("\n@race map=neon seed=new marbles=Ruby,Azure winner=Ruby len=30\n"); }
                if ui.small_button("+ pause").clicked() { ed.text.push_str("\n@wait 1.5\n"); }
            });
            self.script_help(ui);
            if save { self.save_story_edit(); }
            if attach { self.proj_story = self.st_edit.as_ref().unwrap().text.clone(); self.proj_dirty = true; if self.proj_path.is_some() { self.save_project(); } self.say("Attached to this project"); }
            if play { let (t, n) = { let e = self.st_edit.as_ref().unwrap(); (e.text.clone(), e.name.clone()) }; self.start_story(&t, &n); }
            if close { self.st_edit = None; }
            return;
        }
        // ---- library
        card(ui, "Story", |ui| {
            muted(ui, "Typewriter dialogue with a different text sound per character, and races as story moments.");
            if primary(ui, "New story").clicked() { self.st_edit = Some(StoryEdit { path: None, name: "My story".into(), text: projects::STORY_TEMPLATE.into(), saved: String::new() }); }
            ui.horizontal(|ui| {
                if ghost(ui, "Play demo").clicked() { let src = include_str!("../../assets/demo.story"); self.start_story(src, "Demo"); }
                if ghost(ui, "Import file…").clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter("story script", &["story", "txt"]).pick_file() {
                        match std::fs::read_to_string(&f) {
                            Ok(t) => { let n = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("Story".into()); self.st_edit = Some(StoryEdit { path: None, name: n, text: t, saved: String::new() }); self.save_story_edit(); }
                            Err(e) => self.say(format!("Couldn't read that file: {e}")),
                        }
                    }
                }
            });
        });
        if !self.proj_story.trim().is_empty() {
            card(ui, "In this project", |ui| {
                let lines = self.proj_story.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('@')).count();
                muted(ui, format!("{lines} line{} attached to {}", if lines == 1 { "" } else { "s" }, if self.proj_name.is_empty() { "this project" } else { &self.proj_name }));
                ui.horizontal(|ui| {
                    if primary_w(ui, "▶ Play", 80.0).clicked() { let t = self.proj_story.clone(); let n = self.proj_name.clone(); self.start_story(&t, &n); }
                    if ghost(ui, "Edit").clicked() { self.st_edit = Some(StoryEdit { path: None, name: self.proj_name.clone(), text: self.proj_story.clone(), saved: String::new() }); }
                    if ghost(ui, "Detach").clicked() { self.proj_story.clear(); self.proj_dirty = true; if self.proj_path.is_some() { self.save_project(); } }
                });
            });
        }
        let sts = self.lists.as_ref().map(|l| l.1.iter().map(|e| (e.name.clone(), e.path.clone(), e.info.clone(), projects::ago(e.modified))).collect::<Vec<_>>()).unwrap_or_default();
        card(ui, &format!("Saved stories ({})", sts.len()), |ui| {
            if sts.is_empty() { muted(ui, "Nothing saved yet. Hit New story."); }
            for (name, path, info, when) in sts {
                egui::Frame::none().fill(CARD2).rounding(10.0).stroke(egui::Stroke::new(1.0, EDGE)).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new(&name).strong());
                    muted(ui, format!("{info} · {when}"));
                    ui.horizontal_wrapped(|ui| {
                        let text = || std::fs::read_to_string(&path).unwrap_or_default();
                        if ui.small_button("▶ Play").clicked() { let t = text(); self.start_story(&t, &name); }
                        if ui.small_button("Edit").clicked() { let t = text(); self.st_edit = Some(StoryEdit { path: Some(path.clone()), name: name.clone(), saved: t.clone(), text: t }); }
                        if ui.small_button("Duplicate").clicked() { let np = projects::free_path(&projects::stories_dir(), &format!("{name} copy"), "story", None); let _ = std::fs::copy(&path, &np); self.lists = None; }
                        if self.st_del.as_ref() == Some(&path) {
                            if ui.add(egui::Button::new(egui::RichText::new("Really delete").color(egui::Color32::WHITE)).fill(BAD).small()).clicked() { let _ = std::fs::remove_file(&path); self.st_del = None; self.lists = None; }
                        } else if ui.small_button("Delete").clicked() { self.st_del = Some(path.clone()); }
                    });
                });
            }
            if ghost(ui, "Show the folder").clicked() { let _ = open_folder(&projects::stories_dir()); }
        });
        self.script_help(ui);
    }
    fn script_help(&self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(egui::RichText::new("Script format").color(MUTED)).id_salt("sfmt").show(ui, |ui| {
            muted(ui, "@char Ruby color=#ff5a5a blip=square pitch=520 speed=1.0\n@race map=neon seed=ritual marbles=Ruby,Azure winner=Ruby len=40\n@wait 1.5\nRuby: Hello. {p} Ready?\n* Narration starts with a star.\n\n{p} = short pause, {winner} = last race's winner.\nBlip sounds: square, triangle, saw, sine, pulse, noise.");
        });
    }
    fn start_audio_at(&mut self, t: f64) { let keep = self.t; self.t = t; self.start_audio(); self.t = keep; }
    fn start_sing(&mut self) {
        let Some(song) = &self.song else { return };
        let Some(tl) = &self.tl else { return };
        // who leads at each frame (roster index), for the whole race
        let lead: Vec<usize> = tl.lead_a.iter().map(|&a| self.active.get(a as usize).copied().unwrap_or(0)).collect();
        let voices: Vec<ai::Voice> = (0..8).map(|i| ai::Voice { speaker: self.spk[i], clone: self.clones[i].clone() }).collect();
        let (stereo, sr, lyrics) = (song.stereo.clone(), song.sr, self.lyrics.clone());
        let (st, busy, out) = (self.ai_status.clone(), self.ai_busy.clone(), self.ai_out.clone());
        busy.store(true, Ordering::SeqCst);
        *st.lock().unwrap() = "Starting…".into();
        std::thread::spawn(move || {
            let st2 = st.clone();
            let status = move |s: String| *st2.lock().unwrap() = s;
            let leader_at = move |t: f64| lead[((t * 60.0) as usize).min(lead.len().saturating_sub(1))];
            let r = ai::run(&stereo, sr, &lyrics, &voices, &leader_at, &status);
            *out.lock().unwrap() = Some(r);
            busy.store(false, Ordering::SeqCst);
        });
    }
    /// Fully automatic music sync: the race runs the song's length and the winner crosses on the last beat; lead changes land
    /// on bar starts at the song's biggest moments (drops, section changes) in a seeded order; beat kicks come from the beat grid.
    fn auto_sync(&self, n: usize, key: &str) -> (Vec<(f64, usize)>, Option<usize>, f64) {
        let song = self.song.as_ref().unwrap();
        let dur = song.mono.len() as f64 / song.sr as f64;
        let target = song.beats.iter().cloned().filter(|b| *b <= dur - 2.0 && *b > 10.0).last().unwrap_or((dur - 2.5).max(10.0));
        let cuts = marbeat_core::audio::section_times(song, ((dur / 18.0) as usize).clamp(2, 10), 7.0);
        let mut rng = marbeat_core::rng::Rng::new(marbeat_core::rng::hash_seed(&format!("{}|sync", self.eff_seed)));
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() { let j = (rng.next() * (i + 1) as f64) as usize; order.swap(i, j.min(i)); }
        let mut plan: Vec<(f64, usize)> = vec![];
        let mut k = 0;
        for t in cuts.iter().filter(|t| **t < target - 4.0) { k += 1; plan.push((*t, order[k % n])); }
        // never the same marble twice in a row
        for i in 1..plan.len() { if plan[i].1 == plan[i - 1].1 { plan[i].1 = (plan[i].1 + 1) % n; } }
        let winner = plan.last().map(|p| p.1);
        // beat mode builds the race straight from the beats, so the length is simply the song up to its last beat
        let len = target + 2.5;
        (plan, winner, len)
    }
    fn store_path() -> std::path::PathBuf { cfg_path().with_file_name("lyrics.json") }
    fn save_lyrics(&self) {
        if self.song_path.is_empty() { return; }
        let mut j: serde_json::Value = std::fs::read_to_string(Self::store_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(serde_json::json!({}));
        j[&self.song_path] = serde_json::json!(self.lyric_lines.iter().map(|l| serde_json::json!({"t0": l.t0, "t1": l.t1, "words": l.words})).collect::<Vec<_>>());
        let _ = std::fs::write(Self::store_path(), j.to_string());
    }
    fn load_saved_lyrics(&mut self) {
        self.lyric_lines.clear();
        let Ok(t) = std::fs::read_to_string(Self::store_path()) else { return };
        let Ok(j) = serde_json::from_str::<serde_json::Value>(&t) else { return };
        if let Some(a) = j[&self.song_path].as_array() {
            for l in a { self.lyric_lines.push(LyricLine { t0: l["t0"].as_f64().unwrap_or(0.0), t1: l["t1"].as_f64().unwrap_or(0.0), words: l["words"].as_array().map(|w| w.iter().map(|p| (p[0].as_str().unwrap_or("").to_string(), p[1].as_f64().unwrap_or(0.0))).collect()).unwrap_or_default() }); }
        }
    }
    fn start_detect(&mut self, pasted: Option<String>) {
        let Some(song) = &self.song else { return };
        let (stereo, sr) = (song.stereo.clone(), song.sr);
        let (st, busy, out) = (self.lyr_status.clone(), self.lyr_busy.clone(), self.lyr_out.clone());
        busy.store(true, Ordering::SeqCst);
        *st.lock().unwrap() = "Starting…".into();
        std::thread::spawn(move || {
            let st2 = st.clone();
            let r = ai::detect_lyrics(&stereo, sr, pasted.as_deref(), &move |s| *st2.lock().unwrap() = s);
            *out.lock().unwrap() = Some(r);
            busy.store(false, Ordering::SeqCst);
        });
    }
    fn confirm_draft(&mut self) {
        let Some(d) = self.lyr_draft.take() else { return };
        self.lyric_lines = d.iter().enumerate().filter(|(_, l)| !l.text.trim().is_empty()).map(|(i, l)| {
            let t1 = d.get(i + 1).map(|n| n.t0 - 0.05).unwrap_or(l.t1).max(l.t0 + 0.4).min(l.t1.max(l.t0 + 0.4));
            let words: Vec<(String, f64)> = if l.text == l.orig_text { l.orig.iter().map(|(w, t)| (w.clone(), *t + (l.t0 - l.orig.first().map(|f| f.1).unwrap_or(l.t0)))).collect() } else {
                let ws: Vec<&str> = l.text.split_whitespace().collect();
                let tot: usize = ws.iter().map(|w| w.chars().count() + 1).sum();
                let mut acc = 0usize;
                ws.iter().map(|w| { let t = l.t0 + (t1 - l.t0) * acc as f64 / tot.max(1) as f64; acc += w.chars().count() + 1; (w.to_string(), t) }).collect()
            };
            LyricLine { t0: l.t0, t1, words }
        }).collect();
        self.lyrics = d.iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("\n");
        self.save_lyrics();
        self.say("Lyrics saved. They're on screen now.");
    }
    fn rebuild(&mut self) {
        let key = MAPS[self.map].key;
        self.active = match &self.tour {
            Some(t) if t.champion.is_none() => { let g = Self::heat_group(t); g }
            _ => (0..self.n).collect(),
        };
        let n = self.active.len();
        self.eff_seed = match &self.tour { Some(t) if t.champion.is_none() => format!("{}-r{}-h{}", self.seed, t.round, t.heat), _ => self.seed.clone() };
        let auto = self.sync_music && self.song.is_some() && self.story.is_none() && self.tour.is_none();
        let (plan, winner, len) = if auto { self.auto_sync(n, key) } else {
            let plan: Vec<(f64, usize)> = if self.rig { self.plan.iter().cloned().filter(|(_, m)| *m < n).collect() } else { vec![] };
            (plan, if self.rig { self.rig_winner.filter(|w| *w < n) } else { None }, self.len as f64)
        };
        self.cur_plan = plan.clone(); self.cur_winner = winner; self.cur_len = len;
        self.cur_compose = auto;
        self.cur_bars = if auto { marbeat_core::audio::intensity_bars(self.song.as_ref().unwrap()) } else { vec![] };
        if auto { self.len = len as f32; }
        self.tl = Some(build_timeline(&RaceSetup { seed: &self.eff_seed, map: key, n, len, beats: &self.beats, plan: &plan, winner, bars: &self.cur_bars, compose: auto && self.sync_style >= 1, crowd: if self.crowd { 3 } else { 0 }, designed: auto && self.sync_style == 0, race: self.sync_style == 2 }));
        self.chars = self.active.iter().map(|&i| CharStyle { name: self.roster[i].1.clone(), color: self.roster[i].0, label: if self.tour.is_some() { format!("Round {}", self.tour.as_ref().unwrap().round + 1) } else { "Marble".into() } }).collect();
        self.t = 0.0;
        self.playing = true;
        self.start_audio();
    }
    fn per(n: usize) -> usize { let heats = (n + 3) / 4; (n + heats - 1) / heats } // balanced heats of at most 4, never a lone marble
    fn heat_group(t: &Tour) -> Vec<usize> {
        let per = Self::per(t.remaining.len());
        let a = t.heat * per; let b = (a + per).min(t.remaining.len());
        t.remaining[a..b].to_vec()
    }
    fn tour_step(&mut self) {
        // called once a heat's winner card is on screen
        let Some(tl) = &self.tl else { return };
        let w = self.active[tl.sim.winner];
        let name = self.roster[w].1.clone();
        if let Some(t) = &mut self.tour {
            if t.recorded || t.champion.is_some() { return; }
            t.recorded = true;
            t.log.push(format!("R{} heat {}: {} wins", t.round + 1, t.heat + 1, name));
            t.next.push(w);
        }
    }
    fn tour_next(&mut self) {
        if let Some(t) = &mut self.tour {
            if !t.recorded { return; }
            let per = Self::per(t.remaining.len());
            let heats = (t.remaining.len() + per - 1) / per;
            t.heat += 1; t.recorded = false;
            if t.heat >= heats {
                if t.next.len() == 1 { t.champion = Some(t.next[0]); t.log.push(format!("CHAMPION: {}", self.roster[t.next[0]].1)); }
                else { t.remaining = std::mem::take(&mut t.next); t.round += 1; t.heat = 0; }
            }
        }
        let done = self.tour.as_ref().map(|t| t.champion.is_some()).unwrap_or(false);
        if !done { self.rebuild(); }
    }
}

fn dir_size(p: &std::path::Path) -> u64 {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(p) { for e in rd.flatten() { if let Ok(m) = e.metadata() { n += if m.is_dir() { dir_size(&e.path()) } else { m.len() }; } } }
    n
}
fn open_path(p: &std::path::Path) {
    let _ = std::fs::create_dir_all(p);
    #[cfg(windows)] { let _ = std::process::Command::new("explorer").arg(p).spawn(); }
    #[cfg(not(windows))] { let _ = std::process::Command::new("xdg-open").arg(p).spawn(); }
}
fn mb(n: u64) -> String { if n > 1_000_000_000 { format!("{:.1} GB", n as f64 / 1e9) } else { format!("{} MB", n / 1_000_000) } }
fn rand_seed() -> String { format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(7) % 100000) }

impl App {
    fn studio_left(&mut self, ui: &mut egui::Ui, a: &mut Acts) {
        card(ui, "Song", |ui| {
            if primary(ui, "Open a song…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("audio", &["mp3", "wav", "flac", "ogg", "m4a", "aac"]).pick_file() { a.load_path = Some(f.to_string_lossy().to_string()); }
            }
            muted(ui, self.status.clone());
            let rec: Vec<String> = self.recent.iter().filter(|r| **r != self.song_path && std::path::Path::new(r).exists()).take(4).cloned().collect();
            if !rec.is_empty() {
                egui::CollapsingHeader::new(egui::RichText::new("Recent songs").color(MUTED).size(12.5)).id_salt("recent").show(ui, |ui| {
                    for r in rec { let nm = std::path::Path::new(&r).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(); if ui.small_button(nm).on_hover_text(&r).clicked() { a.load_path = Some(r.clone()); } }
                });
            }
            muted(ui, "Tip: drag a song file onto the window.");
            if self.voiced.is_some() { good(ui, "AI voices are on"); }
        });
        card(ui, "Race", |ui| {
            ui.spacing_mut().slider_width = 110.0;
            row_label(ui, "Map");
            egui::ComboBox::from_id_salt("map").width(ui.available_width()).selected_text(MAPS[self.map].label).show_ui(ui, |ui| {
                for (i, m) in MAPS.iter().enumerate() { if ui.selectable_value(&mut self.map, i, m.label).changed() { a.rebuild = true; a.save = true; } }
            });
            row_label(ui, "Seed");
            ui.horizontal(|ui| {
                if ui.add(egui::TextEdit::singleline(&mut self.seed).desired_width(ui.available_width() - 104.0)).lost_focus() { a.rebuild = true; a.save = true; }
                if ghost(ui, "Random").clicked() { self.seed = rand_seed(); a.rebuild = true; a.save = true; }
            });
            if ui.add(egui::Slider::new(&mut self.n, 2..=8).text("marbles")).drag_stopped() { a.rebuild = true; a.save = true; }
            if ui.add(egui::Slider::new(&mut self.len, 15.0..=180.0).text("seconds")).drag_stopped() { a.rebuild = true; }
        });
        card(ui, "Music sync", |ui| {
            if ui.checkbox(&mut self.sync_music, "Sync the whole race to the music").changed() { a.rebuild = true; a.save = true; }
            if self.song.is_none() { muted(ui, "Open a song and the race will fit it automatically."); }
            else if self.sync_music {
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.sync_style == 0, "Track race").clicked() { self.sync_style = 0; a.rebuild = true; a.save = true; }
                    if ui.selectable_label(self.sync_style == 1, "Lane pads").clicked() { self.sync_style = 1; a.rebuild = true; a.save = true; }
                    if ui.selectable_label(self.sync_style == 2, "Lane pads 2").clicked() { self.sync_style = 2; a.rebuild = true; a.save = true; }
                });
                if self.sync_style == 2 { muted(ui, "Lane pads in a race: one tight pack winds down a railed track, everyone on their own beat pattern, shoving and passing each other, longer ramp pads. Every hit is on the beat grid.") }
                else if self.sync_style == 0 { muted(ui, "A winding track of ramps and trampolines. Marbles roll down it, bump and shove each other, leapfrog, and tap the beat on their own grooves. Falls and bounces land on the beat and the winner crosses on the last beat."); }
                else { muted(ui, "Every marble hits pads on its own beat pattern, wandering across the whole track and passing each other. Every hit sits on the beat grid and the winner crosses on the last beat."); }
            }
            else { muted(ui, "Off: you pick the length and use Director for lead changes."); }
        });
        card(ui, "Render", |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(!self.hd, "Fast 540p").clicked() { self.hd = false; a.save = true; }
                if ui.selectable_label(self.hd, "HD 1080p").clicked() { self.hd = true; a.save = true; }
            });
            if self.exporting.load(Ordering::Relaxed) {
                let (p, t) = (self.prog.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed).max(1));
                let eta = self.export_start.map(|s| { let el = s.elapsed().as_secs_f64(); if p > 10 { format!(" · {} left", fmt_time(el * (t - p) as f64 / p as f64)) } else { String::new() } }).unwrap_or_default();
                ui.add(egui::ProgressBar::new(p as f32 / t as f32).text(format!("Rendering {}%{eta}", p * 100 / t)).fill(ACCENT));
                if ghost(ui, "Cancel").clicked() { self.cancel.store(true, Ordering::SeqCst); }
            } else {
                if primary(ui, "Render MP4…").clicked() { a.export = true; }
                if self.ff_found.lock().unwrap().is_none() && !self.ff_busy.load(Ordering::Relaxed) { { ui.label(egui::RichText::new("No usable ffmpeg yet.").color(WARN).size(12.5)); if ghost(ui, "Fix in Settings").clicked() { self.tab = 6; } } }
            }
            if let Some(m) = self.result.lock().unwrap().clone() {
                if m.starts_with("Saved") { good(ui, "Video ready ✔"); muted(ui, m.trim_start_matches("Saved ").to_string());
                    if let Some(p) = &self.last_export { if ghost(ui, "Show in folder").clicked() { if let Some(d) = std::path::Path::new(p).parent() { let _ = open_folder(d); } } }
                } else { ui.label(egui::RichText::new(m).color(BAD).size(12.5)); }
            }
        });
    }

    fn cast_left(&mut self, ui: &mut egui::Ui, a: &mut Acts) {
        card(ui, "Marbles", |ui| {
            let mut ch = false;
            for i in 0..8 {
                ui.horizontal(|ui| {
                    ch |= ui.color_edit_button_srgb(&mut self.roster[i].0).changed();
                    ch |= ui.add(egui::TextEdit::singleline(&mut self.roster[i].1).desired_width(76.0)).changed();
                    ch |= ui.add(egui::DragValue::new(&mut self.spk[i]).range(0..=108).prefix("#")).changed();
                    let lbl = if self.clones[i].is_some() { "clone ✓" } else { "clone" };
                    if ghost(ui, lbl).clicked() {
                        if self.clones[i].is_some() { self.clones[i] = None; ch = true; }
                        else if let Some(f) = rfd::FileDialog::new().add_filter("voice sample", &["wav", "mp3", "flac", "ogg", "m4a"]).pick_file() { self.clones[i] = Some(f.to_string_lossy().to_string()); ch = true; }
                    }
                });
            }
            if ch { a.save = true; for (k, &i) in self.active.iter().enumerate() { if let Some(c) = self.chars.get_mut(k) { c.name = self.roster[i].1.clone(); c.color = self.roster[i].0; } } }
        });
        card(ui, "Voices", |ui| {
            muted(ui, "Pick a voice number (0–108) for each marble. Click “clone” to use a 5–15 second clean recording of someone talking instead. Click again to remove it.");
        });
    }

    fn rig_left(&mut self, ui: &mut egui::Ui, a: &mut Acts) {
        if self.sync_music && self.song.is_some() { card(ui, "Music sync is on", |ui| { muted(ui, "The race is being timed to the song, so these manual settings are ignored. Turn off “Sync the whole race to the music” in Studio to use them."); }); }
        card(ui, "Rig the race", |ui| {
            ui.checkbox(&mut self.rig, "Control who leads and who wins");
            if self.rig {
                let names: Vec<String> = self.roster.iter().take(self.n).map(|r| r.1.clone()).collect();
                muted(ui, "Lead changes: from this time on, this marble is in front.");
                let mut del = None;
                for (k, (t, m)) in self.plan.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(t).speed(0.5).suffix(" s").range(0.0..=300.0));
                        egui::ComboBox::from_id_salt(k).selected_text(names.get(*m).cloned().unwrap_or_default()).show_ui(ui, |ui| { for (i, n) in names.iter().enumerate() { ui.selectable_value(m, i, n); } });
                        if ghost(ui, "X").clicked() { del = Some(k); }
                    });
                }
                if let Some(k) = del { self.plan.remove(k); }
                if ghost(ui, "+ Add lead change").clicked() { let t = self.plan.last().map(|p| p.0 + 6.0).unwrap_or(5.0); self.plan.push((t, 0)); }
                ui.separator();
                ui.horizontal(|ui| {
                    row_label(ui, "Winner");
                    let cur = self.rig_winner.and_then(|w| names.get(w).cloned()).unwrap_or("anyone".into());
                    egui::ComboBox::from_id_salt("win").selected_text(cur).show_ui(ui, |ui| { ui.selectable_value(&mut self.rig_winner, None, "anyone"); for (i, n) in names.iter().enumerate() { ui.selectable_value(&mut self.rig_winner, Some(i), n); } });
                });
            }
            if primary(ui, "Apply and restart").clicked() { a.rebuild = true; }
        });
    }

    fn cup_left(&mut self, ui: &mut egui::Ui, a: &mut Acts) {
        card(ui, "Tournament", |ui| {
            if self.tour.is_none() {
                muted(ui, format!("{} marbles race in heats of up to 4. Winners move on until one is left.", self.n));
                if primary(ui, "Start tournament").clicked() { self.tour = Some(Tour { remaining: (0..self.n).collect(), ..Default::default() }); a.rebuild = true; }
            } else {
                let t = self.tour.clone().unwrap();
                match t.champion {
                    Some(c) => { ui.label(egui::RichText::new(format!("Champion: {}", self.roster[c].1)).size(18.0).strong().color(WARN)); }
                    None => { ui.label(egui::RichText::new(format!("Round {} · heat {}", t.round + 1, t.heat + 1)).strong()); if t.recorded && primary(ui, "Next heat").clicked() { a.tour_next = true; } }
                }
                for l in &t.log { muted(ui, l.clone()); }
                if ghost(ui, "Cancel tournament").clicked() { self.tour = None; a.rebuild = true; }
            }
        });
    }

    fn lyrics_left(&mut self, ui: &mut egui::Ui, a: &mut Acts) {
        card(ui, "On screen", |ui| {
            if ui.checkbox(&mut self.show_lyrics, "Show lyrics on the video").changed() { a.save = true; }
            if self.lyric_lines.is_empty() { muted(ui, "No lyrics yet. Detect them below, then check them."); }
            else {
                good(ui, format!("{} lines ready", self.lyric_lines.len()));
                ui.horizontal(|ui| {
                    if ghost(ui, "Edit").clicked() {
                        self.lyr_draft = Some(self.lyric_lines.iter().map(|l| { let text = l.words.iter().map(|w| w.0.as_str()).collect::<Vec<_>>().join(" "); Draft { t0: l.t0, t1: l.t1, text: text.clone(), orig: l.words.clone(), orig_text: text } }).collect());
                    }
                    if ghost(ui, "Remove").clicked() { self.lyric_lines.clear(); self.save_lyrics(); }
                });
            }
        });
        if self.lyr_draft.is_none() {
            card(ui, "Find the lyrics", |ui| {
                if self.lyr_busy.load(Ordering::Relaxed) {
                    ui.horizontal(|ui| { ui.spinner(); ui.label(self.lyr_status.lock().unwrap().clone()); });
                } else {
                    let have = self.song.is_some();
                    if ui.add_enabled(have, egui::Button::new(egui::RichText::new("Listen and detect lyrics").strong().color(egui::Color32::WHITE)).fill(ACCENT).min_size(egui::vec2(ui.available_width().min(260.0), 36.0))).clicked() { self.start_detect(None); }
                    if !have { muted(ui, "Open a song in Studio first."); }
                    muted(ui, "The AI listens to the vocals and writes down the words with their timing. You check and fix them before anything shows on screen.");
                    ui.collapsing("I already have the lyrics", |ui| {
                        ui.add(egui::TextEdit::multiline(&mut self.lyr_paste).desired_rows(6).desired_width(f32::INFINITY).hint_text("Paste lyrics, one line per row"));
                        if ui.add_enabled(have && !self.lyr_paste.trim().is_empty(), egui::Button::new("Sync these to the song")).clicked() { let t = self.lyr_paste.clone(); self.start_detect(Some(t)); }
                    });
                    let st = self.lyr_status.lock().unwrap().clone(); if !st.is_empty() { muted(ui, st); }
                    muted(ui, "First use downloads the listening models (about 80 MB).");
                }
            });
        }
        if self.lyr_draft.is_some() {
            card(ui, "Check the lyrics", |ui| {
                ui.label(egui::RichText::new("The AI mishears sung words. Fix anything wrong, then confirm.").color(WARN).size(13.0));
                muted(ui, "Press ▶ on a line to hear it. The preview shows the lyrics as they will look.");
                let mut seek = None; let mut del = None;
                if let Some(d) = &mut self.lyr_draft {
                    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                        for (i, l) in d.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                if ghost(ui, "▶").clicked() { seek = Some((l.t0 - 0.6).max(0.0)); }
                                ui.add(egui::DragValue::new(&mut l.t0).speed(0.05).suffix(" s").fixed_decimals(1));
                                if ghost(ui, "Delete").clicked() { del = Some(i); }
                            });
                            ui.add(egui::TextEdit::singleline(&mut l.text).desired_width(f32::INFINITY));
                            ui.add_space(4.0);
                        }
                    });
                    if let Some(i) = del { d.remove(i); }
                    if ghost(ui, "+ Add a line").clicked() { let t = d.last().map(|l| l.t1 + 1.0).unwrap_or(1.0); d.push(Draft { t0: t, t1: t + 3.0, text: String::new(), orig: vec![], orig_text: String::new() }); }
                }
                if let Some(t) = seek { a.seek = Some(t); }
                ui.horizontal(|ui| {
                    if primary_w(ui, "Looks right, show them", 190.0).clicked() { a.confirm_lyrics = true; }
                    if ghost(ui, "Discard").clicked() { self.lyr_draft = None; }
                });
            });
        }
    }

    fn voices_left(&mut self, ui: &mut egui::Ui, a: &mut Acts) {
        if !self.singing {
            card(ui, "Singing is off", |ui| {
                muted(ui, "Marbeat no longer has the marbles sing by default. You can still turn singing on if you want it.");
                if primary(ui, "Turn singing on").clicked() { self.singing = true; a.save = true; }
            });
            return;
        }
        card(ui, "Lyrics", |ui| {
            muted(ui, "One line per row. The AI listens to the song, syncs the words, and whoever is in the lead sings each line.");
            if ui.add(egui::TextEdit::multiline(&mut self.lyrics).desired_rows(12).desired_width(f32::INFINITY).hint_text("Paste lyrics here…")).changed() { a.save = true; }
        });
        card(ui, "Sing", |ui| {
            if self.ai_busy.load(Ordering::Relaxed) {
                ui.horizontal(|ui| { ui.spinner(); ui.label(self.ai_status.lock().unwrap().clone()); });
            } else {
                let ok = self.song.is_some() && !self.lyrics.trim().is_empty();
                if ui.add_enabled(ok, egui::Button::new(egui::RichText::new("Sing the lyrics").strong().color(egui::Color32::WHITE)).fill(ACCENT).min_size(egui::vec2(ui.available_width().min(260.0), 36.0))).clicked() { a.sing_go = true; }
                if self.song.is_none() { muted(ui, "Open a song in Studio first."); }
                if self.voiced.is_some() && ghost(ui, "Back to the original song").clicked() { self.voiced = None; a.restart_audio = true; }
                let st = self.ai_status.lock().unwrap().clone();
                if !st.is_empty() { muted(ui, st); }
                muted(ui, "First run downloads the AI models (about 200 MB).");
            }
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui, a: &mut Acts) {
        let w = ui.available_width();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let side = ((w - 720.0) / 2.0).max(0.0);
            egui::Frame::none().inner_margin(egui::Margin { left: side, right: side, top: 6.0, bottom: 24.0 }).show(ui, |ui| {
                ui.heading("Settings");
                ui.add_space(4.0);

                // ---- updates
                let info = self.upd.lock().unwrap().clone();
                card(ui, "Updates", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("Marbeat v{}", update::VERSION)).size(20.0).strong());
                        let m0 = self.upd_msg.lock().unwrap().clone();
                        if info.is_some() { badge(ui, "Update available", WARN); }
                        else if m0.starts_with("Couldn't") { badge(ui, "Can't check", MUTED); }
                        else if m0.starts_with("You're up to date") { badge(ui, "Up to date", MINT); }
                    });
                    let msg = self.upd_msg.lock().unwrap().clone();
                    if !msg.is_empty() { muted(ui, msg); }
                    if let Some(i) = &info {
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new(format!("New version: v{}", i.version)).strong());
                        if !i.notes.is_empty() { muted(ui, i.notes.clone()); }
                        if self.upd_busy.load(Ordering::Relaxed) {
                            let (g, t) = (self.upd_got.load(Ordering::Relaxed), self.upd_total.load(Ordering::Relaxed).max(1));
                            ui.add(egui::ProgressBar::new(g as f32 / t as f32).text(format!("Downloading {} / {}", mb(g), mb(t))).fill(ACCENT));
                        } else if primary(ui, &format!("Update to v{} and restart", i.version)).clicked() { self.install_update(); }
                    }
                    ui.horizontal(|ui| {
                        let checking = self.upd_checking.load(Ordering::Relaxed);
                        if ui.add_enabled(!checking && !self.upd_busy.load(Ordering::Relaxed), egui::Button::new("Check for updates").min_size(egui::vec2(0.0, 32.0))).clicked() { update::set_url(Some(self.update_url.clone())); self.check_updates(); }
                        if checking { ui.spinner(); }
                    });
                    if ui.checkbox(&mut self.auto_update, "Check for updates when Marbeat starts").changed() { a.save = true; }
                    ui.collapsing("Advanced", |ui| {
                        row_label(ui, "Update source (leave empty for the default)");
                        ui.horizontal(|ui| {
                            if ui.add(egui::TextEdit::singleline(&mut self.update_url).hint_text(update::default_url()).desired_width(ui.available_width() - 80.0)).lost_focus() { update::set_url(Some(self.update_url.clone())); a.save = true; }
                            if ghost(ui, "Reset").clicked() { self.update_url.clear(); update::set_url(None); a.save = true; }
                            if !self.update_url.trim().is_empty() && !update::valid_source(&self.update_url) { ui.label(egui::RichText::new("Must be a link ending in .json. It will be ignored.").color(WARN).size(12.0)); }
                        });
                    });
                });
                ui.add_space(10.0);

                // ---- video
                card(ui, "Video", |ui| {
                    row_label(ui, "Default render quality");
                    ui.horizontal(|ui| {
                        if ui.selectable_label(!self.hd, "Fast  540 × 960").clicked() { self.hd = false; a.save = true; }
                        if ui.selectable_label(self.hd, "HD  1080 × 1920").clicked() { self.hd = true; a.save = true; }
                    });
                    muted(ui, "Renders run at 60 fps with bloom, color grade and mastered audio.");
                });
                ui.add_space(10.0);

                // ---- ffmpeg
                card(ui, "Video encoder (ffmpeg)", |ui| {
                    let found = self.ff_found.lock().unwrap().clone();
                    let busy = self.ff_busy.load(Ordering::Relaxed);
                    ui.horizontal(|ui| {
                        if busy { ui.spinner(); ui.label("Searching this computer…"); }
                        else if found.is_some() { badge(ui, "Found", MINT); }
                        else { badge(ui, "Not found", BAD); }
                    });
                    if let Some(p) = &found { muted(ui, p.clone()); }
                    if found.is_none() && !busy { let log = marbeat_core::export::ffmpeg_log(); if !log.is_empty() { ui.collapsing("What Marbeat found and skipped", |ui| { for l in log { muted(ui, l); } }); } }
                    else if !busy { muted(ui, "Marbeat needs a full ffmpeg to save MP4s. Some copies bundled inside other apps are stripped down and get skipped. Press “Get ffmpeg for me” and Marbeat will download and set one up."); }
                    if self.ff_dl.load(Ordering::Relaxed) {
                        let (g, t) = (self.ff_got.load(Ordering::Relaxed), self.ff_total.load(Ordering::Relaxed).max(1));
                        ui.add(egui::ProgressBar::new(g as f32 / t as f32).text(format!("Downloading {} / {}", mb(g), mb(t))).fill(ACCENT));
                    } else if found.is_none() && !busy {
                        if primary(ui, &format!("Get ffmpeg for me ({})", tools::SIZE_LABEL)).clicked() { self.download_ffmpeg(); }
                    } else if !busy && ghost(ui, &format!("Download a full ffmpeg ({})", tools::SIZE_LABEL)).clicked() { self.download_ffmpeg(); }
                    { let m = self.ff_msg.lock().unwrap().clone(); if !m.is_empty() { muted(ui, m); } }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!busy, egui::Button::new("Find automatically").min_size(egui::vec2(0.0, 32.0))).clicked() {
                            self.ffmpeg_path.clear(); marbeat_core::export::set_ffmpeg_override(None); marbeat_core::export::forget_ffmpeg_cache(); self.detect_ffmpeg(); a.save = true;
                        }
                        if ghost(ui, "Browse…").clicked() {
                            if let Some(f) = rfd::FileDialog::new().set_title("Pick ffmpeg").pick_file() {
                                let p = f.to_string_lossy().to_string();
                                match marbeat_core::export::ffmpeg_problem(&p) {
                                    None => { self.ffmpeg_path = p.clone(); marbeat_core::export::set_ffmpeg_override(Some(p)); self.detect_ffmpeg(); a.save = true; }
                                    Some(why) => self.say(format!("That ffmpeg {why}.")),
                                }
                            }
                        }
                    });
                    if !self.ffmpeg_path.is_empty() { muted(ui, format!("Manual choice: {}", self.ffmpeg_path)); }
                });
                ui.add_space(10.0);

                // ---- AI
                let dir = ai::ai_dir();
                card(ui, "AI models", |ui| {
                    muted(ui, "Voices, lyric listening and vocal separation run on this computer. Models download the first time you use them.");
                    row_label(ui, "Stored in");
                    ui.label(dir.to_string_lossy().to_string());
                    let size = dir_size(&dir);
                    ui.label(format!("{} used", mb(size)));
                    ui.horizontal(|ui| {
                        if ghost(ui, "Open folder").clicked() { open_path(&dir); }
                        if self.confirm_del {
                            if ui.add(egui::Button::new(egui::RichText::new("Really delete?").color(egui::Color32::WHITE)).fill(BAD).min_size(egui::vec2(0.0, 32.0))).clicked() { let _ = std::fs::remove_dir_all(&dir); self.confirm_del = false; self.say("Models deleted. They will download again when needed."); }
                            if ghost(ui, "Keep").clicked() { self.confirm_del = false; }
                        } else if ghost(ui, "Delete downloaded models").clicked() { self.confirm_del = true; }
                    });
                });
                ui.add_space(10.0);

                // ---- data
                card(ui, "Your data", |ui| {
                    row_label(ui, "Settings file");
                    ui.label(cfg_path().to_string_lossy().to_string());
                    ui.horizontal(|ui| {
                        if ghost(ui, "Open folder").clicked() { if let Some(d) = cfg_path().parent() { open_path(d); } }
                        if self.confirm_reset {
                            if ui.add(egui::Button::new(egui::RichText::new("Really reset?").color(egui::Color32::WHITE)).fill(BAD).min_size(egui::vec2(0.0, 32.0))).clicked() {
                                let _ = std::fs::remove_file(cfg_path());
                                self.roster = (0..8).map(|i| (COLS[i], NAMES[i].to_string())).collect(); self.spk = vec![10, 25, 40, 60, 75, 90, 5, 100]; self.clones = vec![None; 8];
                                self.lyrics.clear(); self.seed = "42".into(); self.n = 6; self.map = 0; self.auto_update = true; self.update_url.clear(); update::set_url(None); self.hd = true;
                                self.confirm_reset = false; a.rebuild = true; self.say("Settings reset.");
                            }
                            if ghost(ui, "Keep").clicked() { self.confirm_reset = false; }
                        } else if ghost(ui, "Reset all settings").clicked() { self.confirm_reset = true; }
                    });
                });
                ui.add_space(10.0);

                // ---- music sync
                card(ui, "Music sync", |ui| {
                    if ui.checkbox(&mut self.sync_music, "Sync the whole race to the music automatically").changed() { a.rebuild = true; a.save = true; }
                    muted(ui, "Length, lead changes and the finish are timed to the song you open.");
                    if ui.checkbox(&mut self.crowd, "Extra crowd marbles in the loud parts (busier)").changed() { a.rebuild = true; a.save = true; }
                    row_label(ui, "Picture vs sound timing (preview only)");
                    if ui.add(egui::Slider::new(&mut self.av_offset_ms, -300..=300).suffix(" ms")).changed() { a.save = true; }
                    muted(ui, "If the marbles hit things slightly before or after the beat you hear, nudge this. Rendered videos are always aligned.");
                });
                ui.add_space(10.0);

                // ---- voices
                card(ui, "Voices", |ui| {
                    if ui.checkbox(&mut self.singing, "Let the marbles sing the lyrics (experimental)").changed() { a.save = true; }
                    muted(ui, "Story dialogue always uses each character's own text sound. This only controls the lyric-singing feature.");
                });
                ui.add_space(10.0);

                // ---- about
                card(ui, "About", |ui| {
                    ui.label(egui::RichText::new("Marbeat").strong());
                    muted(ui, "Native Rust build. Physics, rendering, audio and AI all run on your computer.");
                    ui.collapsing("Credits and licenses", |ui| {
                        for l in ["Piper text-to-speech (MIT), VCTK voices", "OpenAI Whisper tiny.en (MIT) for lyric listening", "Spleeter by Deezer (MIT) for vocal separation", "kNN-VC voice conversion (MIT) for voice cloning", "ONNX Runtime (MIT)", "espeak-ng Rust port (GPL-3.0)", "egui, tiny-skia, symphonia, rodio (MIT / Apache-2.0)"] { muted(ui, format!("• {l}")); }
                    });
                });
            });
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().min(0.1);
        self.last = now;
        let end = self.tl.as_ref().map(|t| t.win_time + 5.0).unwrap_or(1.0);
        if self.playing {
            // follow the audio: the picture is driven by how much music has actually been played, so it can't drift away from it
            let audio = self.sink.as_ref().filter(|k| !k.empty() && !k.is_paused()).map(|k| self.audio_t0 + k.get_pos().as_secs_f64());
            match audio {
                Some(at) if (at - (self.t + dt)).abs() > 0.08 => self.t = at,
                Some(at) => self.t += dt + (at - (self.t + dt)) * 0.15,
                None => self.t += dt,
            }
            if self.t > end { self.t = end; self.playing = false; }
        }
        let mut a = Acts::default();
        // ---- story clock
        if self.story.is_some() {
            let in_race = self.story.as_ref().unwrap().player.in_race();
            if !in_race {
                self.playing = false;
                let evs = self.story.as_mut().unwrap().player.update(dt as f32);
                self.story_events(evs);
                let adv = ctx.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Z));
                if adv && self.tab == 5 { let evs = self.story.as_mut().unwrap().player.advance(); self.story_events(evs); }
            } else if let Some(tl) = &self.tl {
                if self.t >= tl.win_time + 3.0 {
                    let w = self.chars.get(tl.sim.winner).map(|c| c.name.clone()).unwrap_or_default();
                    self.playing = false; self.sink = None;
                    let evs = self.story.as_mut().unwrap().player.race_finished(&w);
                    self.story_events(evs);
                }
            }
        }

        // ---- top bar
        egui::TopBottomPanel::top("top").exact_height(54.0).frame(egui::Frame::none().fill(BG2).inner_margin(egui::Margin::symmetric(18.0, 0.0))).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 12.0, ACCENT);
                ui.painter().circle_filled(r.center() + egui::vec2(-4.0, -4.0), 3.6, egui::Color32::from_white_alpha(200));
                ui.label(egui::RichText::new("Marbeat").size(20.0).strong());
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing.x = 1.0;
                for (i, l) in [(8u8, "Projects"), (0u8, "Studio"), (7, "Lyrics"), (1, "Cast"), (2, "Director"), (3, "Tournament"), (5, "Story"), (4, "Voices"), (6, "Settings")] {
                    if tab(ui, self.tab == i, l) { self.tab = i; }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.upd.lock().unwrap().is_some() {
                        if ui.add(egui::Button::new(egui::RichText::new("Update available").strong().color(egui::Color32::BLACK)).fill(WARN).rounding(16.0)).clicked() { self.tab = 6; }
                    }
                    if let Some((m, t0)) = &self.toast { if t0.elapsed().as_secs() < 5 { muted(ui, m.clone()); } }
                    if ui.add(egui::Button::new(egui::RichText::new("?").strong().color(MUTED)).fill(egui::Color32::TRANSPARENT).rounding(16.0)).on_hover_text("Keyboard shortcuts (F1)").clicked() { self.show_keys = !self.show_keys; }
                    ui.label(egui::RichText::new(format!("v{}", update::VERSION)).color(MUTED).size(12.5));
                    let pn = if self.proj_name.is_empty() { "Unsaved".to_string() } else { format!("{}{}", self.proj_name, if self.proj_dirty { " •" } else { "" }) };
                    if ui.add(egui::Button::new(egui::RichText::new(pn).color(TEXT)).fill(CARD2).rounding(16.0)).on_hover_text("Ctrl+S saves. Click for all projects.").clicked() { self.tab = 8; }
                });
            });
        });

        // ---- left panel (everything but Settings)
        if self.tab != 6 {
            egui::SidePanel::left("left").exact_width(318.0).frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(14.0))).show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    match self.tab { 1 => self.cast_left(ui, &mut a), 2 => self.rig_left(ui, &mut a), 3 => self.cup_left(ui, &mut a), 4 => self.voices_left(ui, &mut a), 5 => self.story_left(ui, &mut a), 8 => self.projects_left(ui, &mut a), 7 => self.lyrics_left(ui, &mut a), _ => self.studio_left(ui, &mut a) }
                });
            });
        }

        // ---- central
        let mut scrub_done = false;
        let mut click_adv = false;
        if self.tab == 6 {
            egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(20.0))).show(ctx, |ui| self.settings_page(ui, &mut a));
        } else {
            // draw the frame before the panel that shows it
            self.r.hide_hud = self.story.as_ref().map(|r| !r.player.in_race()).unwrap_or(false);
            if let Some(tl) = &self.tl {
                let empty: Vec<LyricLine> = vec![];
                let drafted: Vec<LyricLine> = match &self.lyr_draft { Some(d) => d.iter().enumerate().filter(|(_, l)| !l.text.trim().is_empty()).map(|(i, l)| {
                    let t1 = d.get(i + 1).map(|n| n.t0 - 0.05).unwrap_or(l.t1).max(l.t0 + 0.4);
                    let ws: Vec<&str> = l.text.split_whitespace().collect(); let tot: usize = ws.iter().map(|w| w.chars().count() + 1).sum(); let mut acc = 0usize;
                    LyricLine { t0: l.t0, t1, words: ws.iter().map(|w| { let t = l.t0 + (t1 - l.t0) * acc as f64 / tot.max(1) as f64; acc += w.chars().count() + 1; (w.to_string(), t) }).collect() }
                }).collect(), None => vec![] };
                let sc = Scene { bars: &self.cur_bars, lyrics: if self.lyr_draft.is_some() { &drafted } else if self.show_lyrics { &self.lyric_lines } else { &empty }, tl, chars: &self.chars, beats: &self.beats, title: &self.race_title, pro_post: false };
                self.r.draw(&sc, (self.t + self.av_offset_ms as f64 / 1000.0).max(0.0));
            }
            if let Some(run) = &self.story {
                if let Some((sp, shown, complete)) = run.player.view() {
                    let full = run.player.full_text();
                    let who = sp.map(|c| (c.name.as_str(), c.color));
                    let who = who.filter(|(n, _)| *n != "Narrator");
                    self.r.dialogue_box(who, &full, shown.chars().count(), complete, ctx.input(|i| i.time));
                }
            }
            let img = egui::ColorImage::from_rgba_premultiplied([self.r.w as usize, self.r.h as usize], self.r.pm.data());
            match &mut self.tex { Some(t) => t.set(img, egui::TextureOptions::LINEAR), None => self.tex = Some(ctx.load_texture("frame", img, egui::TextureOptions::LINEAR)) }
            egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(16.0))).show(ctx, |ui| {
                let avail = ui.available_size();
                let ph = (avail.y - 74.0).max(100.0);
                let s = (avail.x / self.r.w as f32).min(ph / self.r.h as f32);
                let size = egui::vec2(self.r.w as f32 * s, self.r.h as f32 * s);
                ui.allocate_ui_with_layout(egui::vec2(avail.x, ph), egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.add_space(((ph - size.y) / 2.0).max(0.0));
                    if let Some(t) = &self.tex {
                        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                        if resp.clicked() && self.tab == 5 { click_adv = true; }
                        ui.painter().rect_filled(rect.expand(3.0), 14.0, EDGE);
                        egui::Image::new((t.id(), size)).rounding(12.0).paint_at(ui, rect);
                        if self.song.is_none() && self.story.is_none() {
                            let band = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width() * 0.86, 120.0));
                            ui.painter().rect_filled(band, 16.0, egui::Color32::from_black_alpha(190));
                            ui.painter().text(band.center() - egui::vec2(0.0, 18.0), egui::Align2::CENTER_CENTER, "Drop a song here", egui::FontId::proportional(24.0), egui::Color32::WHITE);
                            ui.painter().text(band.center() + egui::vec2(0.0, 18.0), egui::Align2::CENTER_CENTER, "the race syncs to it automatically", egui::FontId::proportional(14.0), MUTED);
                        }
                    }
                });
                ui.add_space(8.0);
                egui::Frame::none().fill(CARD).rounding(12.0).stroke(egui::Stroke::new(1.0, EDGE)).inner_margin(egui::Margin::symmetric(12.0, 8.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if primary_w(ui, if self.playing { "Pause" } else { "Play" }, 84.0).clicked() { self.playing = !self.playing; a.toggled = true; }
                        if ghost(ui, "Restart").clicked() { self.t = 0.0; self.playing = true; a.rebuild = true; }
                        ui.label(egui::RichText::new(fmt_time(self.t)).monospace());
                        ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(60.0);
                        let r = ui.add(egui::Slider::new(&mut self.t, 0.0..=end).show_value(false));
                        if r.drag_stopped() { scrub_done = true; }
                        ui.label(egui::RichText::new(fmt_time(end)).monospace().color(MUTED));
                    });
                });
            });
        }

        // ---- actions
        if a.confirm_lyrics { self.confirm_draft(); }
        if let Some(t) = a.seek.take() { self.t = t; self.playing = true; self.start_audio(); }
        let det = self.lyr_out.lock().unwrap().take();
        if let Some(r) = det {
            match r {
                Ok(lines) => {
                    *self.lyr_status.lock().unwrap() = format!("Heard {} lines. Please check them.", lines.len());
                    self.lyr_draft = Some(lines.into_iter().map(|l| Draft { t0: l.t0, t1: l.t1, text: l.text.clone(), orig: l.words, orig_text: l.text }).collect());
                }
                Err(e) => *self.lyr_status.lock().unwrap() = format!("Couldn't detect lyrics: {e}"),
            }
        }
        if click_adv { if let Some(run) = &mut self.story { let evs = run.player.advance(); self.story_events(evs); } }
        // drop a song (or a .marbeat project / .story script) anywhere on the window
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for f in dropped {
            let ext = f.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            match ext.as_str() {
                "marbeat" => { if self.proj_dirty && self.proj_path.is_some() { self.save_project(); } let d = projects::projects_dir().join(f.file_name().unwrap()); if f.parent() != Some(&projects::projects_dir()) { let _ = std::fs::copy(&f, &d); } self.open_project(&d); self.lists = None; }
                "story" | "txt" => if let Ok(t) = std::fs::read_to_string(&f) { self.tab = 5; self.st_edit = Some(StoryEdit { path: None, name: f.file_stem().unwrap().to_string_lossy().to_string(), text: t, saved: String::new() }); self.save_story_edit(); },
                "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => { a.load_path = Some(f.to_string_lossy().to_string()); self.say("Song loaded"); }
                _ => self.say(format!("Marbeat can't open .{ext} files")),
            }
        }
        let typing = ctx.wants_keyboard_input();
        if !typing && self.story.is_none() && self.tab != 6 {
            let (l, r, home, rk, q) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Home), i.key_pressed(egui::Key::R), i.key_pressed(egui::Key::Questionmark) || i.key_pressed(egui::Key::F1)));
            if l || r { self.t = (self.t + if r { 5.0 } else { -5.0 }).clamp(0.0, end); scrub_done = true; }
            if home || rk { self.t = 0.0; self.playing = true; scrub_done = true; }
            if q { self.show_keys = !self.show_keys; }
        }
        if self.show_keys {
            egui::Window::new("Shortcuts").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                for (k, d) in [("Space", "play / pause"), ("← →", "jump 5 seconds"), ("R / Home", "restart"), ("Ctrl+S", "save project (or story in the editor)"), ("F1 / ?", "this list"), ("Drag a file in", "song, .marbeat project or .story script")] {
                    ui.horizontal(|ui| { ui.add_sized([110.0, 20.0], egui::Label::new(egui::RichText::new(k).monospace().strong())); muted(ui, d); });
                }
                if ghost(ui, "Close").clicked() { self.show_keys = false; }
            });
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            egui::Area::new(egui::Id::new("drop")).fixed_pos(egui::pos2(0.0, 0.0)).order(egui::Order::Foreground).show(ctx, |ui| {
                let r = ctx.screen_rect();
                ui.painter().rect_filled(r, 0.0, egui::Color32::from_black_alpha(170));
                ui.painter().rect_stroke(r.shrink(24.0), 18.0, egui::Stroke::new(3.0, ACCENT));
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "Drop to open", egui::FontId::proportional(34.0), egui::Color32::WHITE);
            });
        }
        let (ctrl_s, space) = ctx.input(|i| (i.modifiers.command && i.key_pressed(egui::Key::S), i.key_pressed(egui::Key::Space)));
        if ctrl_s { if self.tab == 5 && self.st_edit.is_some() { self.save_story_edit(); } else { self.save_project(); } }
        if space && !typing && self.story.is_none() && self.tab != 6 { self.playing = !self.playing; a.toggled = true; }
        if a.save || a.rebuild || a.load_path.is_some() { if self.story.is_none() { self.proj_dirty = true; } }
        if a.save { self.save_cfg(); }
        if self.proj_dirty && self.proj_path.is_some() && self.story.is_none() && !ctx.wants_keyboard_input() && !self.exporting.load(Ordering::Relaxed) { self.save_project(); }
        if a.sing_go { self.start_sing(); }
        if a.restart_audio { let t = self.t; self.start_audio_at(t); }
        let finished = self.ai_out.lock().unwrap().take();
        if let Some(r) = finished {
            match r {
                Ok(o) => { *self.ai_status.lock().unwrap() = format!("Done: {} lines sung ({} words heard)", o.lines, o.heard); self.voiced = Some(o.mixed); let t = self.t; self.start_audio_at(t); }
                Err(e) => *self.ai_status.lock().unwrap() = format!("AI failed: {e}"),
            }
        }
        if a.tour_next { self.tour_next(); a.rebuild = false; }
        if self.tour.is_some() && self.tl.as_ref().map(|t| self.t >= t.win_time + 0.5).unwrap_or(false) { self.tour_step(); }
        if a.export { self.export(); }
        if let Some(p) = a.load_path.take() { self.load(&p); }
        else if a.rebuild { self.rebuild(); }
        if scrub_done { self.start_audio(); }
        if a.toggled { if let Some(k) = &self.sink { if self.playing { k.play() } else { k.pause() } } }
        if self.tab == 6 { ctx.request_repaint_after(std::time::Duration::from_millis(250)); } else { ctx.request_repaint(); }
    }
}


fn open_folder(p: &std::path::Path) -> std::io::Result<std::process::Child> {
    #[cfg(windows)] { std::process::Command::new("explorer").arg(p).spawn() }
    #[cfg(not(windows))] { std::process::Command::new("xdg-open").arg(p).spawn() }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") { println!("marbeat v{}", update::VERSION); return Ok(()); }
    if let Some(i) = args.iter().position(|a| a == "--sing") {
        // headless pipeline test: --sing <song> <lyrics.txt> <out.wav> [clone.wav]
        let song = marbeat_core::audio::load_song(&args[i + 1]).expect("song");
        let lyrics = std::fs::read_to_string(&args[i + 2]).expect("lyrics");
        let voices: Vec<ai::Voice> = (0..2).map(|k| ai::Voice { speaker: if k == 0 { 10 } else { 70 }, clone: if k == 1 { args.get(i + 4).cloned() } else { None } }).collect();
        let t0 = Instant::now();
        let r = ai::run(&song.stereo, song.sr, &lyrics, &voices, &|t| if t < 4.5 { 0 } else { 1 }, &|s| eprintln!("{s}"));
        match r {
            Ok(o) => { eprintln!("ok {} lines, {} words heard, {:?}", o.lines, o.heard, t0.elapsed()); let mut f = std::fs::File::create(&args[i + 3]).unwrap(); let n = o.mixed.len(); use std::io::Write; f.write_all(b"RIFF").unwrap(); f.write_all(&(36 + n as u32 * 2).to_le_bytes()).unwrap(); f.write_all(b"WAVEfmt ").unwrap(); f.write_all(&16u32.to_le_bytes()).unwrap(); f.write_all(&1u16.to_le_bytes()).unwrap(); f.write_all(&2u16.to_le_bytes()).unwrap(); f.write_all(&song.sr.to_le_bytes()).unwrap(); f.write_all(&(song.sr * 4).to_le_bytes()).unwrap(); f.write_all(&4u16.to_le_bytes()).unwrap(); f.write_all(&16u16.to_le_bytes()).unwrap(); f.write_all(b"data").unwrap(); f.write_all(&(n as u32 * 2).to_le_bytes()).unwrap(); for v in o.mixed { f.write_all(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).unwrap(); } }
            Err(e) => { eprintln!("FAILED: {e}"); std::process::exit(1); }
        }
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--detect") {
        let song = marbeat_core::audio::load_song(&args[i + 1]).expect("song");
        let pasted = args.get(i + 2).and_then(|f| std::fs::read_to_string(f).ok());
        match ai::detect_lyrics(&song.stereo, song.sr, pasted.as_deref(), &|s| eprintln!("{s}")) {
            Ok(lines) => for l in lines { println!("{:6.2}-{:6.2}  {}", l.t0, l.t1, l.text); },
            Err(e) => { eprintln!("FAILED: {e}"); std::process::exit(1); }
        }
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--sync-test") {
        let mut a = App::new();
        for map in 0..MAPS.len() {
            a.map = map; a.load(&args[i + 1]);
            let dur = a.song.as_ref().map(|s| s.mono.len() as f64 / s.sr as f64).unwrap_or(0.0);
            let last = a.song.as_ref().and_then(|s| s.beats.iter().cloned().filter(|b| *b <= dur - 2.0).last()).unwrap_or(0.0);
            let tl = a.tl.as_ref().unwrap();
            let held: Vec<bool> = a.cur_plan.iter().map(|(t, m)| { let f = (((t + 2.0) * 60.0) as usize).min(tl.lead_a.len() - 1); tl.lead_a[f] as usize == *m }).collect();
            println!("landed {:?} (held 2s later: {:?})", tl.landed, held);
            println!("{:9} song {:.1}s last beat {:.2}s -> winner crosses {:.2}s (len {:.1}), lead changes {:?}, winner idx {:?} sim.winner {}", MAPS[map].key, dur, last, tl.win_time, a.cur_len, a.cur_plan.iter().map(|p| (format!("{:.1}", p.0), p.1)).collect::<Vec<_>>(), a.cur_winner, tl.sim.winner);
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--get-ffmpeg") {
        match tools::get_ffmpeg(|g, t| { if g % (1 << 22) < (1 << 17) { eprintln!("{g}/{t}"); } }) { Ok(p) => { println!("installed {}", p.display()); println!("problem: {:?}", marbeat_core::export::ffmpeg_problem(&p.to_string_lossy())); } Err(e) => { eprintln!("FAILED: {e}"); std::process::exit(1); } }
        return Ok(());
    }
    if args.iter().any(|a| a == "--self-update") {
        match update::check() { Ok(Some(i)) => { println!("updating to v{}", i.version); if let Err(e) = update::install(&i, |_, _| {}) { eprintln!("failed: {e}"); std::process::exit(1); } } Ok(None) => println!("up to date"), Err(e) => { eprintln!("check failed: {e}"); std::process::exit(1); } }
        return Ok(());
    }
    eframe::run_native("Marbeat", eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 800.0]).with_min_inner_size([900.0, 640.0]), ..Default::default() }, Box::new(|cc| { ui::theme(&cc.egui_ctx); Ok(Box::new(App::new())) }))
}
