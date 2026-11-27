mod ai;
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
    story: Option<StoryRun>,
    story_name: String,
    race_title: String,
}

struct StoryRun { player: story::Player, saved_roster: Vec<([u8; 3], String)>, saved_n: usize, saved_rig: (bool, Option<usize>), blip_n: u32 }

#[derive(Default)]
struct Acts { rebuild: bool, load_path: Option<String>, toggled: bool, export: bool, save: bool, tour_next: bool, sing_go: bool, restart_audio: bool, seek: Option<f64> }

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
            roster: (0..8).map(|i| (COLS[i], NAMES[i].to_string())).collect(), rig: false, plan: vec![], rig_winner: None, active: vec![], eff_seed: String::new(), tour: None, tab: 0, spk: vec![10, 25, 40, 60, 75, 90, 5, 100], clones: vec![None; 8], lyrics: String::new(), voiced: None, ai_status: Default::default(), ai_busy: Default::default(), ai_out: Default::default(), auto_update: true, update_url: String::new(), ffmpeg_path: String::new(), ff_found: Default::default(), ff_busy: Default::default(), upd_checking: Default::default(), upd_got: Default::default(), upd_total: Default::default(), toast: None, confirm_del: false, confirm_reset: false, ff_dl: Default::default(), ff_got: Default::default(), ff_total: Default::default(), ff_msg: Default::default(), singing: false, story: None, story_name: String::new(), race_title: "Marbeat".into() };
        a.load_cfg();
        update::set_url(Some(a.update_url.clone()));
        marbeat_core::export::set_ffmpeg_override(Some(a.ffmpeg_path.clone()));
        if a.auto_update { a.check_updates(); }
        if std::env::var("MARBEAT_STORY").is_ok() { let src = include_str!("../../assets/demo.story"); a.start_story(src, "Demo"); }
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
                self.rebuild();
            }
            Err(e) => self.status = format!("Couldn't load: {e}"),
        }
    }
    fn export(&mut self) {
        let Some(out) = rfd::FileDialog::new().add_filter("MP4 video", &["mp4"]).set_file_name("marbeat.mp4").save_file() else { return };
        let out = out.to_string_lossy().to_string();
        let (key, seed, n, len, beats, hd) = (MAPS[self.map].key, self.eff_seed.clone(), self.active.len(), self.len as f64, self.beats.clone(), self.hd);
        let plan: Vec<(f64, usize)> = if self.rig { self.plan.iter().cloned().filter(|(_, m)| *m < n).collect() } else { vec![] };
        let rw = if self.rig { self.rig_winner.filter(|w| *w < n) } else { None };
        let song = self.song.as_ref().map(|s| Song { sr: s.sr, mono: vec![], stereo: self.voiced.clone().unwrap_or_else(|| s.stereo.clone()), beats: vec![], bpm: s.bpm });
        let chars: Vec<(String, [u8; 3])> = self.chars.iter().map(|c| (c.name.clone(), c.color)).collect();
        let title_s = self.race_title.clone();
        let (prog, total, cancel, busy, res) = (self.prog.clone(), self.total.clone(), self.cancel.clone(), self.exporting.clone(), self.result.clone());
        cancel.store(false, Ordering::SeqCst);
        busy.store(true, Ordering::SeqCst);
        prog.store(0, Ordering::SeqCst);
        std::thread::spawn(move || {
            let tl = build_timeline(&RaceSetup { seed: &seed, map: key, n, len, beats: &beats, plan: &plan, winner: rw });
            let cs: Vec<CharStyle> = chars.iter().map(|(n, c)| CharStyle { name: n.clone(), color: *c, label: "Marble".into() }).collect();
            let sc = Scene { tl: &tl, chars: &cs, beats: &beats, title: "Marbeat", pro_post: true };
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
                if let Some(x) = j["update_url"].as_str() { self.update_url = x.into(); }
                if let Some(x) = j["ffmpeg_path"].as_str() { self.ffmpeg_path = x.into(); }
                if let Some(x) = j["singing"].as_bool() { self.singing = x; }
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
        let j = serde_json::json!({"roster": self.roster.iter().map(|(c, n)| serde_json::json!({"name": n, "color": c})).collect::<Vec<_>>(), "auto_update": self.auto_update, "update_url": self.update_url, "ffmpeg_path": self.ffmpeg_path, "hd": self.hd, "singing": self.singing, "speakers": self.spk, "clones": self.clones, "lyrics": self.lyrics, "seed": self.seed, "n": self.n, "map": self.map});
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
                Err(_) => *m.lock().unwrap() = "Couldn't reach the update server. Are you online?".into(),
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
    fn story_left(&mut self, ui: &mut egui::Ui, a: &mut Acts) {
        let _ = a;
        card(ui, "Story", |ui| {
            if let Some(run) = &self.story {
                let (i, n) = run.player.progress();
                ui.label(egui::RichText::new(self.story_name.clone()).strong());
                ui.add(egui::ProgressBar::new(i as f32 / n.max(1) as f32).text(format!("{} / {}", i, n)).fill(ACCENT));
                muted(ui, "Space, Enter or a click moves on. A race can't be skipped.");
                if ghost(ui, "Stop story").clicked() { self.stop_story(); }
            } else {
                muted(ui, "Plays a story script: typewriter dialogue with a different text sound per character, and races as story moments.");
                if primary(ui, "Play the demo script").clicked() { let src = include_str!("../../assets/demo.story"); self.start_story(src, "Demo"); }
                if ghost(ui, "Open a script…").clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter("story script", &["story", "txt"]).pick_file() {
                        match std::fs::read_to_string(&f) { Ok(t) => { let n = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("Story".into()); self.start_story(&t, &n); } Err(e) => self.say(format!("Couldn't read that file: {e}")) }
                    }
                }
            }
        });
        card(ui, "Script format", |ui| {
            muted(ui, "@char Ruby color=#ff5a5a blip=square pitch=520 speed=1.0\n@race map=neon seed=ritual marbles=Ruby,Azure winner=Ruby len=40\n@wait 1.5\nRuby: Hello. {p} Ready?\n* Narration starts with a star.\n\n{p} = short pause, {winner} = last race's winner.\nBlip sounds: square, triangle, saw, sine, pulse, noise.".replace("\\n", "\n"));
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
    fn rebuild(&mut self) {
        let key = MAPS[self.map].key;
        self.active = match &self.tour {
            Some(t) if t.champion.is_none() => { let g = Self::heat_group(t); g }
            _ => (0..self.n).collect(),
        };
        let n = self.active.len();
        self.eff_seed = match &self.tour { Some(t) if t.champion.is_none() => format!("{}-r{}-h{}", self.seed, t.round, t.heat), _ => self.seed.clone() };
        let plan: Vec<(f64, usize)> = if self.rig { self.plan.iter().cloned().filter(|(_, m)| *m < n).collect() } else { vec![] };
        let winner = if self.rig { self.rig_winner.filter(|w| *w < n) } else { None };
        self.tl = Some(build_timeline(&RaceSetup { seed: &self.eff_seed, map: key, n, len: self.len as f64, beats: &self.beats, plan: &plan, winner }));
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
        card(ui, "Render", |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(!self.hd, "Fast 540p").clicked() { self.hd = false; a.save = true; }
                if ui.selectable_label(self.hd, "HD 1080p").clicked() { self.hd = true; a.save = true; }
            });
            if self.exporting.load(Ordering::Relaxed) {
                let (p, t) = (self.prog.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed).max(1));
                ui.add(egui::ProgressBar::new(p as f32 / t as f32).text(format!("Rendering {}%", p * 100 / t)).fill(ACCENT));
                if ghost(ui, "Cancel").clicked() { self.cancel.store(true, Ordering::SeqCst); }
            } else {
                if primary(ui, "Render MP4…").clicked() { a.export = true; }
                if self.ff_found.lock().unwrap().is_none() && !self.ff_busy.load(Ordering::Relaxed) { { ui.label(egui::RichText::new("No usable ffmpeg yet.").color(WARN).size(12.5)); if ghost(ui, "Fix in Settings").clicked() { self.tab = 6; } } }
            }
            if let Some(m) = self.result.lock().unwrap().clone() { muted(ui, m); }
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
                        if ghost(ui, "✕").clicked() { del = Some(k); }
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
            self.t += dt;
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
                ui.add_space(18.0);
                for (i, l) in ["Studio", "Cast", "Director", "Tournament", "Voices", "Story", "Settings"].iter().enumerate() {
                    if tab(ui, self.tab == i as u8, l) { self.tab = i as u8; }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.upd.lock().unwrap().is_some() {
                        if ui.add(egui::Button::new(egui::RichText::new("Update available").strong().color(egui::Color32::BLACK)).fill(WARN).rounding(16.0)).clicked() { self.tab = 6; }
                    }
                    if let Some((m, t0)) = &self.toast { if t0.elapsed().as_secs() < 5 { muted(ui, m.clone()); } }
                    ui.label(egui::RichText::new(format!("v{}", update::VERSION)).color(MUTED).size(12.5));
                });
            });
        });

        // ---- left panel (everything but Settings)
        if self.tab != 6 {
            egui::SidePanel::left("left").exact_width(318.0).frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(14.0))).show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    match self.tab { 1 => self.cast_left(ui, &mut a), 2 => self.rig_left(ui, &mut a), 3 => self.cup_left(ui, &mut a), 4 => self.voices_left(ui, &mut a), 5 => self.story_left(ui, &mut a), _ => self.studio_left(ui, &mut a) }
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
                let sc = Scene { tl, chars: &self.chars, beats: &self.beats, title: &self.race_title, pro_post: false };
                self.r.draw(&sc, self.t);
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
        if click_adv { if let Some(run) = &mut self.story { let evs = run.player.advance(); self.story_events(evs); } }
        if a.save { self.save_cfg(); }
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
