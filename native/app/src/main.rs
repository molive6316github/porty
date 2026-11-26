mod update;
use eframe::egui;
use marbeat_core::course::MAPS;
use marbeat_core::render::*;
use marbeat_core::sim::*;
use std::sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Mutex};
use std::time::Instant;
use marbeat_core::audio::{load_song, Song};
use rodio::{buffer::SamplesBuffer, OutputStream, OutputStreamHandle, Sink};

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
}

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
            roster: (0..8).map(|i| (COLS[i], NAMES[i].to_string())).collect(), rig: false, plan: vec![], rig_winner: None, active: vec![], eff_seed: String::new(), tour: None, tab: 0 };
        a.load_cfg();
        if let Ok(t) = std::env::var("MARBEAT_TAB") { a.tab = t.parse().unwrap_or(0); }
        update::cleanup_old();
        { let (u, m) = (a.upd.clone(), a.upd_msg.clone()); std::thread::spawn(move || match update::check() { Ok(Some(i)) => *u.lock().unwrap() = Some(i), Ok(None) => {}, Err(_) => *m.lock().unwrap() = "Offline: couldn't check for updates".to_string() }); }
        a.rebuild();
        a
    }
    fn start_audio(&mut self) {
        self.sink = None;
        if let (Some(song), Some((_, h))) = (&self.song, &self._stream) {
            if let Ok(sink) = Sink::try_new(h) {
                let skip = (self.t * song.sr as f64) as usize * 2;
                if skip < song.stereo.len() {
                    sink.append(SamplesBuffer::new(2, song.sr, song.stereo[skip..].to_vec()));
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
        let song = self.song.as_ref().map(|s| Song { sr: s.sr, mono: vec![], stereo: s.stereo.clone(), beats: vec![], bpm: s.bpm });
        let chars: Vec<(String, [u8; 3])> = self.chars.iter().map(|c| (c.name.clone(), c.color)).collect();
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
                if let Some(x) = j["seed"].as_str() { self.seed = x.into(); }
                if let Some(x) = j["n"].as_u64() { self.n = (x as usize).clamp(2, 8); }
                if let Some(x) = j["map"].as_u64() { self.map = (x as usize).min(MAPS.len() - 1); }
            }
        }
        self.rebuild();
    }
    fn save_cfg(&self) {
        let j = serde_json::json!({"roster": self.roster.iter().map(|(c, n)| serde_json::json!({"name": n, "color": c})).collect::<Vec<_>>(), "seed": self.seed, "n": self.n, "map": self.map});
        let _ = std::fs::write(cfg_path(), j.to_string());
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
        let mut rebuild = false;
        let mut load_path: Option<String> = None;
        let mut toggled = false;
        let mut export = false;
        let mut save = false;
        let mut tour_next = false;
        egui::SidePanel::left("ctl").exact_width(220.0).show(ctx, |ui| {
            ui.heading("Marbeat");
            ui.label(format!("native Rust build · v{}", update::VERSION));
            let info = self.upd.lock().unwrap().clone();
            if let Some(i) = info {
                ui.colored_label(egui::Color32::from_rgb(120, 255, 140), format!("Update v{} available", i.version));
                if !i.notes.is_empty() { ui.small(&i.notes); }
                if self.upd_busy.load(Ordering::Relaxed) { ui.spinner(); }
                else if ui.button("Update & restart").clicked() {
                    self.upd_busy.store(true, Ordering::SeqCst);
                    let (m, b, ii) = (self.upd_msg.clone(), self.upd_busy.clone(), i.clone());
                    std::thread::spawn(move || { *m.lock().unwrap() = "Downloading update…".into(); if let Err(e) = update::install(&ii, |_, _| {}) { *m.lock().unwrap() = format!("Update failed: {e}"); } b.store(false, Ordering::SeqCst); });
                }
            }
            { let m = self.upd_msg.lock().unwrap(); if !m.is_empty() { ui.small(m.as_str()); } }
            ui.horizontal(|ui| { for (i, l) in ["Race", "Cast", "Director", "Tournament"].iter().enumerate() { ui.selectable_value(&mut self.tab, i as u8, *l); } });
            ui.separator();
            match self.tab {
                1 => {
                    let mut ch = false;
                    for (c, n) in self.roster.iter_mut() { ui.horizontal(|ui| { ch |= ui.color_edit_button_srgb(c).changed(); ch |= ui.add(egui::TextEdit::singleline(n).desired_width(110.0)).changed(); }); }
                    if ch { save = true; for (k, &i) in self.active.iter().enumerate() { if let Some(c) = self.chars.get_mut(k) { c.name = self.roster[i].1.clone(); c.color = self.roster[i].0; } } }
                    return;
                }
                2 => {
                    ui.checkbox(&mut self.rig, "Rig the race");
                    let names: Vec<String> = self.roster.iter().take(self.n).map(|r| r.1.clone()).collect();
                    let mut del = None;
                    for (k, (t, m)) in self.plan.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(t).speed(0.5).suffix("s").range(0.0..=300.0));
                            egui::ComboBox::from_id_salt(k).selected_text(names.get(*m).cloned().unwrap_or_default()).show_ui(ui, |ui| { for (i, n) in names.iter().enumerate() { ui.selectable_value(m, i, n); } });
                            if ui.small_button("x").clicked() { del = Some(k); }
                        });
                    }
                    if let Some(k) = del { self.plan.remove(k); }
                    if ui.button("+ lead change").clicked() { let t = self.plan.last().map(|p| p.0 + 6.0).unwrap_or(5.0); self.plan.push((t, 0)); }
                    ui.horizontal(|ui| {
                        ui.label("Winner");
                        let cur = self.rig_winner.and_then(|w| names.get(w).cloned()).unwrap_or("anyone".into());
                        egui::ComboBox::from_id_salt("win").selected_text(cur).show_ui(ui, |ui| { ui.selectable_value(&mut self.rig_winner, None, "anyone"); for (i, n) in names.iter().enumerate() { ui.selectable_value(&mut self.rig_winner, Some(i), n); } });
                    });
                    if ui.button("Apply (restart race)").clicked() { rebuild = true; }
                    return;
                }
                3 => {
                    if self.tour.is_none() {
                        ui.label(format!("{} marbles, heats of up to 4, winners advance.", self.n));
                        if ui.button("Start tournament").clicked() { self.tour = Some(Tour { remaining: (0..self.n).collect(), ..Default::default() }); rebuild = true; }
                    } else {
                        let t = self.tour.clone().unwrap();
                        match t.champion { Some(c) => { ui.colored_label(egui::Color32::GOLD, format!("Champion: {}", self.roster[c].1)); }
                            None => { ui.label(format!("Round {} · heat {}", t.round + 1, t.heat + 1)); if t.recorded && ui.button("Next heat").clicked() { tour_next = true; } } }
                        for l in &t.log { ui.small(l); }
                        if ui.button("Cancel tournament").clicked() { self.tour = None; rebuild = true; }
                    }
                    return;
                }
                _ => {}
            }
            if ui.button("Open song…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("audio", &["mp3", "wav", "flac", "ogg", "m4a", "aac"]).pick_file() { load_path = Some(f.to_string_lossy().to_string()); }
            }
            ui.label(&self.status);
            ui.separator();
            egui::ComboBox::from_label("Map").selected_text(MAPS[self.map].label).show_ui(ui, |ui| {
                for (i, m) in MAPS.iter().enumerate() { if ui.selectable_value(&mut self.map, i, m.label).changed() { rebuild = true; } }
            });
            ui.horizontal(|ui| { ui.label("Seed"); if ui.text_edit_singleline(&mut self.seed).lost_focus() { rebuild = true; } });
            if ui.add(egui::Slider::new(&mut self.n, 2..=8).text("Marbles")).drag_stopped() { rebuild = true; }
            if ui.add(egui::Slider::new(&mut self.len, 15.0..=180.0).text("Seconds")).drag_stopped() { rebuild = true; }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(if self.playing { "Pause" } else { "Play" }).clicked() { self.playing = !self.playing; toggled = true; }
                if ui.button("Restart").clicked() { self.t = 0.0; self.playing = true; rebuild = true; }
            });
            ui.add(egui::Slider::new(&mut self.t, 0.0..=end).text("t"));
            ui.separator();
            ui.checkbox(&mut self.hd, "HD 1080x1920");
            if self.exporting.load(Ordering::Relaxed) {
                let (p, t) = (self.prog.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed).max(1));
                ui.add(egui::ProgressBar::new(p as f32 / t as f32).text(format!("Rendering {}%", p * 100 / t)));
                if ui.button("Cancel").clicked() { self.cancel.store(true, Ordering::SeqCst); }
            } else if ui.button("Render MP4…").clicked() { export = true; }
            if let Some(m) = self.result.lock().unwrap().clone() { ui.label(m); }
            if ui.button("New random seed").clicked() { self.seed = format!("{}", (now.elapsed().subsec_nanos() ^ 0x5bd1e995) % 100000); rebuild = true; }
        });
        if save { self.save_cfg(); }
        if tour_next { self.tour_next(); rebuild = false; }
        if self.tour.is_some() && self.tl.as_ref().map(|t| self.t >= t.win_time + 0.5).unwrap_or(false) { self.tour_step(); }
        if export { self.export(); }
        if let Some(p) = load_path { self.load(&p); }
        else if rebuild { self.rebuild(); }
        if toggled { if let Some(k) = &self.sink { if self.playing { k.play() } else { k.pause() } } }
        if let Some(tl) = &self.tl {
            let sc = Scene { tl, chars: &self.chars, beats: &self.beats, title: "Marbeat", pro_post: false };
            self.r.draw(&sc, self.t);
        }
        let img = egui::ColorImage::from_rgba_premultiplied([self.r.w as usize, self.r.h as usize], self.r.pm.data());
        match &mut self.tex { Some(t) => t.set(img, egui::TextureOptions::LINEAR), None => self.tex = Some(ctx.load_texture("frame", img, egui::TextureOptions::LINEAR)) }
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(t) = &self.tex {
                let avail = ui.available_size();
                let s = (avail.x / self.r.w as f32).min(avail.y / self.r.h as f32);
                ui.centered_and_justified(|ui| { ui.image((t.id(), egui::vec2(self.r.w as f32 * s, self.r.h as f32 * s))); });
            }
        });
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") { println!("marbeat v{}", update::VERSION); return Ok(()); }
    if args.iter().any(|a| a == "--self-update") {
        match update::check() { Ok(Some(i)) => { println!("updating to v{}", i.version); if let Err(e) = update::install(&i, |_, _| {}) { eprintln!("failed: {e}"); std::process::exit(1); } } Ok(None) => println!("up to date"), Err(e) => { eprintln!("check failed: {e}"); std::process::exit(1); } }
        return Ok(());
    }
    eframe::run_native("Marbeat", eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([760.0, 780.0]), ..Default::default() }, Box::new(|_| Ok(Box::new(App::new()))))
}
