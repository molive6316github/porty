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
}

impl App {
    fn new() -> Self {
        let mut a = App { map: 0, seed: "42".into(), n: 6, len: 40.0, tl: None, beats: (0..2000).map(|i| i as f64 * 0.5).collect(), chars: vec![], r: Renderer::new(405, 720), tex: None, t: 0.0, playing: true, last: Instant::now(), song: None, status: "No song loaded".into(), _stream: OutputStream::try_default().ok(), sink: None, prog: Default::default(), total: Default::default(), cancel: Default::default(), exporting: Default::default(), result: Default::default(), hd: true, upd: Default::default(), upd_msg: Default::default(), upd_busy: Default::default() };
        update::cleanup_old();
        { let (u, m) = (a.upd.clone(), a.upd_msg.clone()); std::thread::spawn(move || match update::check() { Ok(Some(i)) => *u.lock().unwrap() = Some(i), Ok(None) => {}, Err(e) => *m.lock().unwrap() = format!("Update check failed: {e}") }); }
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
        let (key, seed, n, len, beats, hd) = (MAPS[self.map].key, self.seed.clone(), self.n, self.len as f64, self.beats.clone(), self.hd);
        let song = self.song.as_ref().map(|s| Song { sr: s.sr, mono: vec![], stereo: s.stereo.clone(), beats: vec![], bpm: s.bpm });
        let chars: Vec<(String, [u8; 3])> = self.chars.iter().map(|c| (c.name.clone(), c.color)).collect();
        let (prog, total, cancel, busy, res) = (self.prog.clone(), self.total.clone(), self.cancel.clone(), self.exporting.clone(), self.result.clone());
        cancel.store(false, Ordering::SeqCst);
        busy.store(true, Ordering::SeqCst);
        prog.store(0, Ordering::SeqCst);
        std::thread::spawn(move || {
            let tl = build_timeline(&RaceSetup { seed: &seed, map: key, n, len, beats: &beats, plan: &[], winner: None });
            let cs: Vec<CharStyle> = chars.iter().map(|(n, c)| CharStyle { name: n.clone(), color: *c, label: "Marble".into() }).collect();
            let sc = Scene { tl: &tl, chars: &cs, beats: &beats, title: "Marbeat", pro_post: true };
            let r = marbeat_core::export::export_mp4(&sc, song.as_ref(), &out, if hd { 2 } else { 1 }, |f, t| { prog.store(f, Ordering::Relaxed); total.store(t, Ordering::Relaxed); !cancel.load(Ordering::Relaxed) });
            *res.lock().unwrap() = Some(match r { Ok(()) => format!("Saved {out}"), Err(e) => format!("Export failed: {}", e.lines().next().unwrap_or("")) });
            busy.store(false, Ordering::SeqCst);
        });
    }
    fn rebuild(&mut self) {
        let key = MAPS[self.map].key;
        self.tl = Some(build_timeline(&RaceSetup { seed: &self.seed, map: key, n: self.n, len: self.len as f64, beats: &self.beats, plan: &[], winner: None }));
        self.chars = (0..self.n).map(|i| CharStyle { name: NAMES[i].into(), color: COLS[i], label: "Marble".into() }).collect();
        self.t = 0.0;
        self.playing = true;
        self.start_audio();
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
            ui.separator();
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
