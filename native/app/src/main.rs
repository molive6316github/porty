use eframe::egui;
use marbeat_core::course::MAPS;
use marbeat_core::render::*;
use marbeat_core::sim::*;
use std::time::Instant;

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
}

impl App {
    fn new() -> Self {
        let mut a = App { map: 0, seed: "42".into(), n: 6, len: 40.0, tl: None, beats: (0..2000).map(|i| i as f64 * 0.5).collect(), chars: vec![], r: Renderer::new(405, 720), tex: None, t: 0.0, playing: true, last: Instant::now() };
        a.rebuild();
        a
    }
    fn rebuild(&mut self) {
        let key = MAPS[self.map].key;
        self.tl = Some(build_timeline(&RaceSetup { seed: &self.seed, map: key, n: self.n, len: self.len as f64, beats: &self.beats, plan: &[], winner: None }));
        self.chars = (0..self.n).map(|i| CharStyle { name: NAMES[i].into(), color: COLS[i], label: "Marble".into() }).collect();
        self.t = 0.0;
        self.playing = true;
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
        egui::SidePanel::left("ctl").exact_width(220.0).show(ctx, |ui| {
            ui.heading("Marbeat");
            ui.label("native Rust build");
            ui.separator();
            egui::ComboBox::from_label("Map").selected_text(MAPS[self.map].label).show_ui(ui, |ui| {
                for (i, m) in MAPS.iter().enumerate() { if ui.selectable_value(&mut self.map, i, m.label).changed() { rebuild = true; } }
            });
            ui.horizontal(|ui| { ui.label("Seed"); if ui.text_edit_singleline(&mut self.seed).lost_focus() { rebuild = true; } });
            if ui.add(egui::Slider::new(&mut self.n, 2..=8).text("Marbles")).drag_stopped() { rebuild = true; }
            if ui.add(egui::Slider::new(&mut self.len, 15.0..=180.0).text("Seconds")).drag_stopped() { rebuild = true; }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(if self.playing { "Pause" } else { "Play" }).clicked() { self.playing = !self.playing; }
                if ui.button("Restart").clicked() { self.t = 0.0; self.playing = true; }
            });
            ui.add(egui::Slider::new(&mut self.t, 0.0..=end).text("t"));
            if ui.button("New random seed").clicked() { self.seed = format!("{}", (now.elapsed().subsec_nanos() ^ 0x5bd1e995) % 100000); rebuild = true; }
        });
        if rebuild { self.rebuild(); }
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
    eframe::run_native("Marbeat", eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([760.0, 780.0]), ..Default::default() }, Box::new(|_| Ok(Box::new(App::new()))))
}
