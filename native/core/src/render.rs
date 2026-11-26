//! The "pro" look, drawn natively with tiny-skia (a pure-Rust 2D rasteriser). No GPU, no window: it renders to a pixel buffer.
use crate::course::*;
use crate::sim::*;
use std::collections::HashMap;
use tiny_skia::*;

pub struct CharStyle {
    pub name: String,
    pub color: [u8; 3],
    pub label: String, // second line in the banner (e.g. the voice)
}

pub struct Scene<'a> {
    pub tl: &'a Timeline,
    pub chars: &'a [CharStyle],
    pub beats: &'a [f64],
    pub title: &'a str,
    pub pro_post: bool, // bloom + grade + grain (the final render; the live preview skips them)
}

pub struct Renderer {
    pub w: u32,
    pub h: u32,
    pub pm: Pixmap,
    bold: fontdue::Font,
    sprites: HashMap<[u8; 3], Pixmap>,
    glow: HashMap<i32, Pixmap>,
    grain: Vec<u8>,
    frame_no: u64,
}

fn h01(i: f64, k: f64) -> f64 {
    let x = (i * 127.1 + k * 311.7).sin() * 43758.5453;
    x - x.floor()
}
fn col(c: [f32; 3], a: f32) -> Color {
    Color::from_rgba(c[0] / 255.0, c[1] / 255.0, c[2] / 255.0, a.clamp(0.0, 1.0)).unwrap()
}
fn colu(c: [u8; 3], a: f32) -> Color {
    col([c[0] as f32, c[1] as f32, c[2] as f32], a)
}
fn mix(c: [u8; 3], target: f32, a: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| c[i] as f32 + (target - c[i] as f32) * a)
}
fn paint_c(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}
fn circle(cx: f32, cy: f32, r: f32) -> Path {
    PathBuilder::from_circle(cx, cy, r.max(0.01)).unwrap()
}
fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = r * 0.5523;
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(x + w - r, y);
    p.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    p.line_to(x + w, y + h - r);
    p.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    p.line_to(x + r, y + h);
    p.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    p.line_to(x, y + r);
    p.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    p.close();
    p.finish().unwrap()
}
fn radial(cx: f32, cy: f32, r: f32, stops: Vec<(f32, Color)>) -> Shader<'static> {
    RadialGradient::new(
        Point::from_xy(cx, cy),
        Point::from_xy(cx, cy),
        r.max(0.01),
        stops.into_iter().map(|(p, c)| GradientStop::new(p, c)).collect(),
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap()
}
fn fill(pm: &mut Pixmap, path: &Path, paint: &Paint, t: Transform) {
    pm.fill_path(path, paint, FillRule::Winding, t, None);
}
fn stroke_line(pm: &mut Pixmap, x1: f32, y1: f32, x2: f32, y2: f32, w: f32, c: Color, t: Transform, blend: BlendMode) {
    let mut pb = PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);
    let path = pb.finish().unwrap();
    let mut p = paint_c(c);
    p.blend_mode = blend;
    let s = Stroke { width: w, line_cap: LineCap::Round, ..Stroke::default() };
    pm.stroke_path(&path, &p, &s, t, None);
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
}

impl Renderer {
    pub fn new(w: u32, h: u32) -> Self {
        let bold = fontdue::Font::from_bytes(include_bytes!("../../assets/DejaVuSans-Bold.ttf") as &[u8], fontdue::FontSettings::default()).unwrap();
        let mut s = 0x9E3779B9u32;
        let grain = (0..256 * 256)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s & 255) as u8
            })
            .collect();
        Renderer { w, h, pm: Pixmap::new(w, h).unwrap(), bold, sprites: HashMap::new(), glow: HashMap::new(), grain, frame_no: 0 }
    }

    fn base(&self) -> Transform {
        Transform::from_scale(self.w as f32 / W as f32, self.w as f32 / W as f32)
    }

    // ---------- text ----------
    pub fn text_width(&self, s: &str, px: f32) -> f32 {
        s.chars().map(|c| self.bold.metrics(c, px).advance_width).sum()
    }
    /// logical coordinates (540x960); baseline at y
    pub fn text(&mut self, s: &str, x: f32, y: f32, px: f32, c: [u8; 3], a: f32, align: Align, outline: bool) {
        let k = self.w as f32 / W as f32;
        let (pxs, ox) = (px * k, if align == Align::Center { x - self.text_width(s, px) / 2.0 } else { x });
        let mut offs: Vec<(f32, f32, [u8; 3], f32)> = Vec::new();
        if outline {
            let r = (px * 0.07).max(1.2);
            for i in 0..8 {
                let ang = i as f32 * std::f32::consts::FRAC_PI_4;
                offs.push((ang.cos() * r, ang.sin() * r, [0, 0, 0], a * 0.85));
            }
        }
        offs.push((0.0, 0.0, c, a));
        for (dx, dy, cc, aa) in offs {
            let mut cx = (ox + dx) * k;
            let by = ((y + dy) * k).round();
            for ch in s.chars() {
                let (m, bmp) = self.bold.rasterize(ch, pxs);
                let (gx, gy) = ((cx + m.xmin as f32).round() as i32, by as i32 - m.height as i32 - m.ymin);
                for yy in 0..m.height {
                    for xx in 0..m.width {
                        let cov = bmp[yy * m.width + xx] as f32 / 255.0 * aa;
                        if cov <= 0.0 {
                            continue;
                        }
                        let (px_, py_) = (gx + xx as i32, gy + yy as i32);
                        if px_ < 0 || py_ < 0 || px_ >= self.w as i32 || py_ >= self.h as i32 {
                            continue;
                        }
                        let i = (py_ as usize * self.w as usize + px_ as usize) * 4;
                        let d = self.pm.data_mut();
                        for ch3 in 0..3 {
                            d[i + ch3] = (cc[ch3] as f32 * cov + d[i + ch3] as f32 * (1.0 - cov)) as u8;
                        }
                        d[i + 3] = 255;
                    }
                }
                cx += m.advance_width;
            }
        }
    }

    // ---------- sprites ----------
    fn marble_sprite(&mut self, c: [u8; 3]) -> &Pixmap {
        self.sprites.entry(c).or_insert_with(|| {
            let s = 128u32;
            let mut pm = Pixmap::new(s, s).unwrap();
            let (r, cc) = (s as f32 / 2.0 - 6.0, s as f32 / 2.0);
            let body = Paint { shader: RadialGradient::new(Point::from_xy(cc - r * 0.35, cc - r * 0.4), Point::from_xy(cc, cc), r,
                vec![GradientStop::new(0.0, col(mix(c, 255.0, 0.7), 1.0)), GradientStop::new(0.42, colu(c, 1.0)), GradientStop::new(1.0, col(mix(c, 0.0, 0.62), 1.0))],
                SpreadMode::Pad, Transform::identity()).unwrap(), anti_alias: true, ..Paint::default() };
            fill(&mut pm, &circle(cc, cc, r), &body, Transform::identity());
            // rim light
            let rim = Paint { shader: radial(cc + r * 0.55, cc + r * 0.6, r * 0.95, vec![(0.0, col(mix(c, 255.0, 0.45), 0.5)), (1.0, Color::from_rgba8(0, 0, 0, 0))]), anti_alias: true, ..Paint::default() };
            let mut mask = Mask::new(s, s).unwrap();
            mask.fill_path(&circle(cc, cc, r), FillRule::Winding, true, Transform::identity());
            pm.fill_rect(Rect::from_xywh(0.0, 0.0, s as f32, s as f32).unwrap(), &rim, Transform::identity(), Some(&mask));
            // glass reflection band
            let mut pb = PathBuilder::new();
            let (rx, ry, rad) = (cc - r * 0.1, cc - r * 0.1, r * 0.7);
            for i in 0..=24 {
                let a = std::f32::consts::PI * (1.06 + 0.49 * i as f32 / 24.0);
                let (px, py) = (rx + a.cos() * rad, ry + a.sin() * rad);
                if i == 0 { pb.move_to(px, py) } else { pb.line_to(px, py) }
            }
            let st = Stroke { width: r * 0.15, line_cap: LineCap::Round, ..Stroke::default() };
            pm.stroke_path(&pb.finish().unwrap(), &paint_c(Color::from_rgba8(255, 255, 255, 80)), &st, Transform::identity(), Some(&mask));
            // specular dot
            let sp = PathBuilder::from_oval(Rect::from_xywh(cc - r * 0.38 - r * 0.17, cc - r * 0.43 - r * 0.1, r * 0.34, r * 0.2).unwrap()).unwrap();
            fill(&mut pm, &sp, &paint_c(Color::from_rgba8(255, 255, 255, 245)), Transform::from_rotate_at(-40.0, cc - r * 0.38, cc - r * 0.43));
            let ol = Stroke { width: 2.4, ..Stroke::default() };
            pm.stroke_path(&circle(cc, cc, r), &paint_c(col(mix(c, 0.0, 0.75), 0.85)), &ol, Transform::identity(), None);
            pm
        })
    }
    fn glow_sprite(&mut self, hue: i32) -> &Pixmap {
        self.glow.entry(hue).or_insert_with(|| {
            let s = 96u32;
            let mut pm = Pixmap::new(s, s).unwrap();
            let c = hsl(hue as f32, 90.0, 60.0);
            let p = Paint { shader: radial(48.0, 48.0, 48.0, vec![(0.0, col(c, 0.6)), (1.0, col(c, 0.0))]), ..Paint::default() };
            pm.fill_rect(Rect::from_xywh(0.0, 0.0, s as f32, s as f32).unwrap(), &p, Transform::identity(), None);
            pm
        })
    }
    fn draw_sprite(&mut self, which: u8, key: [u8; 3], hue: i32, cx: f32, cy: f32, size: f32, alpha: f32, blend: BlendMode, t: Transform) {
        let sp = if which == 0 { self.marble_sprite(key).clone() } else { self.glow_sprite(hue).clone() };
        let k = size / sp.width() as f32;
        let tf = t.pre_translate(cx - size / 2.0, cy - size / 2.0).pre_scale(k, k);
        self.pm.draw_pixmap(0, 0, sp.as_ref(), &PixmapPaint { opacity: alpha, blend_mode: blend, quality: FilterQuality::Bilinear }, tf, None);
    }

    // ---------- the frame ----------
    pub fn draw(&mut self, sc: &Scene, t: f64) {
        self.frame_no += 1;
        let tl = sc.tl;
        let (n, sim) = (tl.n, &tl.sim);
        let pal = tl.course.pal;
        let base = self.base();
        // background gradient
        let bg = Paint { shader: LinearGradient::new(Point::from_xy(0.0, 0.0), Point::from_xy(0.0, H as f32),
            vec![GradientStop::new(0.0, col(pal.bg1, 1.0)), GradientStop::new(1.0, col(pal.bg2, 1.0))], SpreadMode::Pad, Transform::identity()).unwrap(), ..Paint::default() };
        self.pm.fill_rect(Rect::from_xywh(0.0, 0.0, self.w as f32, self.h as f32).unwrap(), &bg, Transform::from_scale(1.0, self.h as f32 / H as f32), None);

        // positions, interpolated between the 60 Hz physics steps
        let fa = (t * FPS).max(0.0);
        let f = (fa.floor() as usize).min(tl.frames);
        let sf_f = (fa * tl.factor).clamp(0.0, sim.frames as f64 - 1.0);
        let (sf0, u) = (sf_f.floor() as usize, sf_f - sf_f.floor());
        let sf1 = (sf0 + 1).min(sim.frames - 1);
        let p: Vec<(f64, f64)> = (0..n).map(|i| { let (a, b) = (sim.at(sf0, i), sim.at(sf1, i)); (a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u) }).collect();
        let cam = sim.cam[sf0] as f64 * (1.0 - u) + sim.cam[sf1] as f64 * u;
        let ts = sf_f / FPS;
        let score = |i: usize| if sim.fins[i] >= 0.0 && sim.fins[i] <= ts { 1e9 - sim.fins[i] } else { p[i].1 };
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| score(b).partial_cmp(&score(a)).unwrap());
        let leader = tl.lead_a[f.min(tl.lead_a.len() - 1)] as usize;
        let pulse = match sc.beats.partition_point(|&b| b <= t) { 0 => 0.0, k => (-(t - sc.beats[k - 1]) * 9.0).exp() };
        let wz = (t - tl.win_time).max(0.0);
        let ez = 1.0 - (1.0 - (wz / 1.3).min(1.0)).powi(3);
        let zoom = ((1.0 + pulse * 0.012) * (1.0 + 0.1 * ez)) as f32;
        let shake = if sc.pro_post { ((t * 93.0).sin() * pulse * 3.0) as f32 } else { 0.0 };
        let shake_y = if sc.pro_post { ((t * 77.0).cos() * pulse * 3.0) as f32 } else { 0.0 };

        self.background(&tl.course, cam, t, pulse, base);

        // world transform: zoom about the centre, camera follows the leader
        let off = H * 0.42 - cam;
        let world = base.pre_translate(W as f32 / 2.0, H as f32 * 0.5).pre_scale(zoom, zoom).pre_translate(-(W as f32) / 2.0, -(H as f32) * 0.5).pre_translate(shake, off as f32 + shake_y);
        let (top, bot) = (cam - H * 0.42 - 60.0, cam + H * 0.58 + 60.0);
        // walls and lane lines
        let wall = paint_c(Color::from_rgba8(26, 26, 46, 255));
        self.pm.fill_rect(Rect::from_xywh(-20.0, top as f32, 22.0, (bot - top) as f32).unwrap(), &wall, world, None);
        self.pm.fill_rect(Rect::from_xywh(W as f32 - 2.0, top as f32, 22.0, (bot - top) as f32).unwrap(), &wall, world, None);
        let mut gy = (top / 80.0).floor() * 80.0;
        while gy < bot {
            stroke_line(&mut self.pm, 0.0, gy as f32, W as f32, gy as f32, 1.0, Color::from_rgba8(255, 255, 255, 10), world, BlendMode::SourceOver);
            gy += 80.0;
        }
        // finish line
        let fy = tl.course.finish_y;
        if fy > top && fy < bot {
            for i in 0..27 { for r in 0..2 {
                let c = if (i + r) % 2 == 1 { Color::WHITE } else { Color::from_rgba8(17, 17, 17, 255) };
                self.pm.fill_rect(Rect::from_xywh(i as f32 * 20.0, fy as f32 + r as f32 * 10.0, 20.0, 10.0).unwrap(), &paint_c(c), world, None);
            } }
        }
        // the course
        for s in &tl.course.segs {
            if s.y1.max(s.y2) < top - 30.0 || s.y1.min(s.y2) > bot + 30.0 { continue; }
            let hue = ((pal.hb + ((s.y1 * 0.17) % pal.hs as f64) as f32) % 360.0) as f32;
            self.seg_pro(s, hue, pulse as f32, world);
        }
        for sp in &tl.course.spins {
            if sp.cy < top - 120.0 || sp.cy > bot + 120.0 { continue; }
            let a = sp.ph + sp.w * ts;
            let (c, s) = ((a.cos() * sp.len) as f32, (a.sin() * sp.len) as f32);
            let (cx, cy, r) = (sp.cx as f32, sp.cy as f32, sp.r as f32);
            stroke_line(&mut self.pm, cx - c, cy - s, cx + c, cy + s, r * 5.0, Color::from_rgba8(255, 255, 255, 30), world, BlendMode::SourceOver);
            stroke_line(&mut self.pm, cx - c, cy - s, cx + c, cy + s, r * 2.0, col(pal.sp, 1.0), world, BlendMode::SourceOver);
            stroke_line(&mut self.pm, cx - c, cy - s - r * 0.3, cx + c, cy + s - r * 0.3, r * 0.6, Color::from_rgba8(255, 255, 255, 204), world, BlendMode::SourceOver);
            fill(&mut self.pm, &circle(cx, cy, 5.0), &paint_c(Color::WHITE), world);
        }
        // comet trails
        for &i in &order {
            for k in 1..=8usize {
                let q = sim.at(sf0.saturating_sub(k * 2), i);
                if q.1 < top - 30.0 || q.1 > bot + 30.0 { continue; }
                let a = 0.34 * (1.0 - k as f32 / 9.0);
                fill(&mut self.pm, &circle(q.0 as f32, q.1 as f32, RAD as f32 * (1.0 - 0.06 * k as f32)), &paint_c(colu(sc.chars[i].color, a)), world);
            }
        }
        // marbles (the leader on top)
        for &i in order.iter().rev() {
            let (x, y) = (p[i].0 as f32, p[i].1 as f32);
            if (y as f64) < top - 30.0 || (y as f64) > bot + 30.0 { continue; }
            let c = sc.chars[i].color;
            let r = RAD as f32;
            let sh = PathBuilder::from_oval(Rect::from_xywh(x + r * 0.28 - r * 1.02, y + r * 0.5 - r * 0.78, r * 2.04, r * 1.56).unwrap()).unwrap();
            fill(&mut self.pm, &sh, &paint_c(Color::from_rgba8(0, 0, 0, 87)), world);
            if i == leader {
                let k = 1.0 + 0.12 * (ts * 9.0).sin() as f32;
                self.draw_sprite(1, [0; 3], (pal.hb as i32 / 5) * 5, x, y, r * 5.2 * k, 1.0, BlendMode::Plus, world);
                let aura = Paint { shader: radial(x, y, r * 2.6 * k, vec![(0.0, colu(c, 0.5)), (1.0, colu(c, 0.0))]), blend_mode: BlendMode::Plus, ..Paint::default() };
                fill(&mut self.pm, &circle(x, y, r * 2.6 * k), &aura, world);
            }
            self.draw_sprite(0, c, 0, x, y, 2.0 * r * (128.0 / 116.0), 1.0, BlendMode::SourceOver, world);
        }
        // overtake flash
        let mut tc = -1.0;
        for k in 0..45usize {
            let ff = f as i64 - k as i64;
            if ff >= 1 && tl.lead_a[ff as usize] != tl.lead_a[ff as usize - 1] { tc = ff as f64 / FPS; break; }
        }
        if tc >= 0.0 && t - tc < 0.7 && t > 0.3 {
            let (a, k) = ((t - tc) as f32, 1.0 - (t - tc) as f32 / 0.7);
            let (lx, ly) = (p[leader].0 as f32, p[leader].1 as f32);
            let st = Stroke { width: 5.0 * k + 1.0, ..Stroke::default() };
            self.pm.stroke_path(&circle(lx, ly, RAD as f32 + 8.0 + a * 190.0), &paint_c(colu(sc.chars[leader].color, k)), &st, world, None);
        }
        // leader tag + crown
        let (lx, ly) = (p[leader].0 as f32, p[leader].1 as f32);
        let nm = &sc.chars[leader].name;
        let tw = self.text_width(nm, 14.0) + 22.0;
        let fillp = paint_c(colu(sc.chars[leader].color, 0.92));
        fill(&mut self.pm, &rrect(lx - tw / 2.0, ly - RAD as f32 - 34.0, tw, 22.0, 11.0), &fillp, world);
        self.crown(lx, ly - RAD as f32 - 38.0, 9.0, world);
        // the leader's name inside the pill: drawn in screen space at the transformed position
        let (sx, sy) = self.to_screen(lx, ly - RAD as f32 - 18.0, zoom, shake, off as f32 + shake_y);
        self.text(nm, sx, sy, 14.0, [255, 255, 255], 1.0, Align::Center, false);

        self.hud(sc, &p, &order, leader, t, pulse);
        self.confetti(tl.win_time, t, sc);
        self.title_card(t, sc.title);
        // vignette
        let vg = Paint { shader: radial(W as f32 / 2.0, H as f32 / 2.0, H as f32 * 0.72, vec![(0.0, Color::from_rgba8(0, 0, 0, 0)), (0.39, Color::from_rgba8(0, 0, 0, 0)), (1.0, Color::from_rgba8(0, 0, 0, 128))]), ..Paint::default() };
        self.pm.fill_rect(Rect::from_xywh(0.0, 0.0, W as f32, H as f32).unwrap(), &vg, base, None);
        if sc.pro_post { self.post(); }
    }

    fn to_screen(&self, x: f32, y: f32, zoom: f32, sx: f32, oy: f32) -> (f32, f32) {
        // world -> logical screen coordinates (inverse of the world transform used above)
        let (cx, cy) = (W as f32 / 2.0, H as f32 * 0.5);
        let (wx, wy) = (x + sx, y + oy);
        ((wx - cx) * zoom + cx, (wy - cy) * zoom + cy)
    }

    fn crown(&mut self, x: f32, y: f32, s: f32, t: Transform) {
        let mut pb = PathBuilder::new();
        for (i, (dx, dy)) in [(-1.0, 0.0), (-0.85, -0.9), (-0.4, -0.4), (0.0, -1.1), (0.4, -0.4), (0.85, -0.9), (1.0, 0.0)].iter().enumerate() {
            if i == 0 { pb.move_to(x + dx * s, y + dy * s) } else { pb.line_to(x + dx * s, y + dy * s) }
        }
        pb.close();
        let path = pb.finish().unwrap();
        fill(&mut self.pm, &path, &paint_c(Color::from_rgba8(255, 211, 77, 255)), t);
        self.pm.stroke_path(&path, &paint_c(Color::from_rgba8(120, 70, 0, 204)), &Stroke { width: 1.5, ..Stroke::default() }, t, None);
    }

    fn seg_pro(&mut self, s: &Seg, hue: f32, pulse: f32, t: Transform) {
        let c = hsl(hue, 85.0, 62.0);
        match s.kind {
            Kind::Peg => {
                let hq = (hue as i32 / 5) * 5;
                self.draw_sprite(1, [0; 3], hq, s.x1 as f32, s.y1 as f32, s.r as f32 * 6.8, (0.7 + pulse * 0.3).min(1.0), BlendMode::Plus, t);
                let body = hsl(hue, 80.0, 58.0 + pulse * 18.0);
                fill(&mut self.pm, &circle(s.x1 as f32, s.y1 as f32, s.r as f32 + pulse * 2.0), &paint_c(col(body, 1.0)), t);
                fill(&mut self.pm, &circle(s.x1 as f32 - s.r as f32 * 0.3, s.y1 as f32 - s.r as f32 * 0.3, s.r as f32 * 0.32), &paint_c(Color::from_rgba8(255, 255, 255, 204)), t);
            }
            Kind::Bump => {
                let h2 = (hue + 120.0) % 360.0;
                let (x, y, r) = (s.x1 as f32, s.y1 as f32, s.r as f32);
                let st = Stroke { width: 3.0, ..Stroke::default() };
                self.pm.stroke_path(&circle(x, y, r + 6.0 + (1.0 - pulse) * 26.0), &paint_c(col(hsl(h2, 90.0, 70.0), pulse * 0.85)), &st, t, None);
                let glow = Paint { shader: radial(x, y, r * 2.2, vec![(0.0, col(hsl(h2, 90.0, 55.0), 0.5)), (1.0, col(hsl(h2, 90.0, 55.0), 0.0))]), blend_mode: BlendMode::Plus, ..Paint::default() };
                fill(&mut self.pm, &circle(x, y, r * 2.2), &glow, t);
                let bg = Paint { shader: RadialGradient::new(Point::from_xy(x - r * 0.3, y - r * 0.35), Point::from_xy(x, y), r,
                    vec![GradientStop::new(0.0, col(hsl(h2, 90.0, 78.0 + pulse * 10.0), 1.0)), GradientStop::new(0.6, col(hsl(h2, 85.0, 48.0 + pulse * 14.0), 1.0)), GradientStop::new(1.0, col(hsl(h2, 85.0, 28.0), 1.0))],
                    SpreadMode::Pad, Transform::identity()).unwrap(), anti_alias: true, ..Paint::default() };
                fill(&mut self.pm, &circle(x, y, r + pulse * 3.0), &bg, t);
                let hl = PathBuilder::from_oval(Rect::from_xywh(x - r * 0.32 - r * 0.28, y - r * 0.38 - r * 0.16, r * 0.56, r * 0.32).unwrap()).unwrap();
                fill(&mut self.pm, &hl, &paint_c(Color::from_rgba8(255, 255, 255, 140)), t.pre_rotate_at(-40.0, x - r * 0.32, y - r * 0.38));
            }
            Kind::Ramp => {
                let (x1, y1, x2, y2, r) = (s.x1 as f32, s.y1 as f32, s.x2 as f32, s.y2 as f32, s.r as f32);
                stroke_line(&mut self.pm, x1, y1, x2, y2, r * 5.2, col(hsl(hue, 90.0, 60.0), 0.2), t, BlendMode::SourceOver);
                stroke_line(&mut self.pm, x1, y1, x2, y2, r * 2.0, col(c, 1.0), t, BlendMode::SourceOver);
                stroke_line(&mut self.pm, x1, y1 - r * 0.35, x2, y2 - r * 0.35, r * 0.55, Color::from_rgba(1.0, 1.0, 1.0, 0.55 + pulse * 0.3).unwrap(), t, BlendMode::SourceOver);
            }
        }
    }

    fn background(&mut self, course: &Course, cam: f64, t: f64, pulse: f64, base: Transform) {
        let pal = course.pal;
        for i in 0..4 {
            let fi = i as f64;
            let x = W * (0.1 + 0.8 * h01(fi, 1.0));
            let y = (((h01(fi, 2.0) * H * 2.2 - cam * 0.12) % (H * 1.7)) + H * 1.7) % (H * 1.7) - H * 0.35;
            let rad = 240.0 + h01(fi, 3.0) * 160.0;
            let c = hsl(pal.hb + i as f32 * 47.0, 75.0, 48.0);
            let p = Paint { shader: radial(x as f32, y as f32, rad as f32, vec![(0.0, col(c, (0.13 + pulse * 0.05) as f32)), (1.0, col(c, 0.0))]), blend_mode: BlendMode::Plus, ..Paint::default() };
            fill(&mut self.pm, &circle(x as f32, y as f32, rad as f32), &p, base);
        }
        let key = course.map_key;
        for i in 0..64 {
            let fi = i as f64;
            let d = 0.25 + 0.75 * h01(fi, 4.0);
            let x0 = h01(fi, 5.0) * W;
            let sp = match key { "spinner" => -26.0, "slide" => 22.0, "funnel" => -18.0, "pachinko" => 8.0, _ => 5.0 } * d;
            let y = (((h01(fi, 6.0) * H * 1.5 - cam * d * 0.45 + t * sp) % (H * 1.2)) + H * 1.2) % (H * 1.2) - H * 0.1;
            let x = x0 + (t * 0.7 * d + fi).sin() * 10.0 * d;
            let tw = 0.55 + 0.45 * (t * 2.2 + fi * 3.1).sin();
            let size = (0.8 + h01(fi, 7.0) * 2.0) * d * if key == "funnel" { 2.0 } else { 1.0 };
            if key == "funnel" {
                let st = Stroke { width: 1.0, ..Stroke::default() };
                self.pm.stroke_path(&circle(x as f32, y as f32, (size * 2.4) as f32), &paint_c(col(hsl(pal.hb, 80.0, 75.0), (0.28 * d) as f32)), &st, base, None);
            } else {
                let c = match key {
                    "spinner" => col(hsl(20.0 + h01(fi, 8.0) as f32 * 30.0, 100.0, 60.0), (0.55 * d * tw) as f32),
                    "pachinko" => col(hsl(if h01(fi, 8.0) > 0.5 { 330.0 } else { 50.0 }, 100.0, 75.0), (0.7 * d * tw) as f32),
                    _ => Color::from_rgba(1.0, 1.0, 1.0, (0.55 * d * tw) as f32).unwrap(),
                };
                fill(&mut self.pm, &circle(x as f32, y as f32, size as f32), &paint_c(c), base);
            }
        }
    }

    fn hud(&mut self, sc: &Scene, p: &[(f64, f64)], order: &[usize], leader: usize, t: f64, pulse: f64) {
        let base = self.base();
        let (tl, n) = (sc.tl, sc.tl.n);
        let ch = &sc.chars[leader];
        let c = ch.color;
        // top banner
        fill(&mut self.pm, &rrect(18.0, 16.0, W as f32 - 36.0, 86.0, 43.0), &paint_c(Color::from_rgba8(12, 12, 24, 158)), base);
        self.pm.stroke_path(&rrect(18.0, 16.0, W as f32 - 36.0, 86.0, 43.0), &paint_c(colu(c, 0.55)), &Stroke { width: 2.0, ..Stroke::default() }, base, None);
        fill(&mut self.pm, &circle(62.0, 59.0, 26.0), &paint_c(colu(c, 1.0)), base);
        let initial: String = ch.name.chars().next().map(|x| x.to_uppercase().collect()).unwrap_or_default();
        self.text(&initial, 62.0, 69.0, 28.0, [255, 255, 255], 1.0, Align::Center, false);
        self.text("L E A D I N G", 102.0, 40.0, 12.0, [200, 200, 210], 0.6, Align::Left, false);
        self.text(&ch.name, 102.0, 74.0, 34.0, [255, 255, 255], 1.0, Align::Left, false);
        self.text(&ch.label, 102.0, 94.0, 13.0, [220, 220, 230], 0.55, Align::Left, false);
        for i in 0..6 {
            let hgt = (6.0 + pulse * 22.0 * (0.5 + 0.5 * (i as f64 * 1.9).sin()) + (t * 9.0 + i as f64 * 1.7).sin().abs() * 16.0) as f32;
            fill(&mut self.pm, &rrect(W as f32 - 46.0 - i as f32 * 12.0, 59.0 - hgt / 2.0, 6.0, hgt, 3.0), &paint_c(colu(c, 0.9)), base);
        }
        // progress rail
        fill(&mut self.pm, &rrect(W as f32 - 20.0, 130.0, 6.0, H as f32 - 340.0, 3.0), &paint_c(Color::from_rgba8(255, 255, 255, 36)), base);
        for i in 0..n {
            let u = (p[i].1 / tl.course.finish_y).clamp(0.0, 1.0) as f32;
            let yy = 130.0 + u * (H as f32 - 340.0);
            fill(&mut self.pm, &circle(W as f32 - 17.0, yy, 9.0), &paint_c(colu(sc.chars[i].color, 0.35)), base);
            fill(&mut self.pm, &circle(W as f32 - 17.0, yy, 5.0), &paint_c(colu(sc.chars[i].color, 1.0)), base);
        }
        // leaderboard
        for (k, &i) in order.iter().enumerate() {
            let y = H as f32 - 34.0 - (n - k) as f32 * 30.0;
            let top = k == 0;
            fill(&mut self.pm, &rrect(14.0, y - 18.0, 168.0, 26.0, 13.0), &paint_c(if top { colu(sc.chars[i].color, 0.35) } else { Color::from_rgba8(12, 12, 24, 140) }), base);
            if top { self.pm.stroke_path(&rrect(14.0, y - 18.0, 168.0, 26.0, 13.0), &paint_c(colu(sc.chars[i].color, 0.8)), &Stroke { width: 2.0, ..Stroke::default() }, base, None); }
            self.text(&(k + 1).to_string(), 32.0, y, 15.0, [255, 255, 255], 0.55, Align::Center, false);
            fill(&mut self.pm, &circle(52.0, y - 6.0, 6.0), &paint_c(colu(sc.chars[i].color, 1.0)), base);
            self.text(&sc.chars[i].name, 66.0, y, 15.0, [255, 255, 255], 1.0, Align::Left, false);
        }
        // winner card
        if t >= tl.win_time {
            let a = ((t - tl.win_time) / 0.45).min(1.0) as f32;
            let wi = tl.sim.winner;
            let wc = sc.chars[wi].color;
            fill(&mut self.pm, &PathBuilder::from_rect(Rect::from_xywh(0.0, 0.0, W as f32, H as f32).unwrap()), &paint_c(Color::from_rgba(0.0, 0.0, 0.0, 0.5 * a).unwrap()), base);
            let s = 0.85 + 0.15 * (a * 1.4).min(1.0);
            let (cx, cy) = (W as f32 / 2.0, H as f32 * 0.42);
            let card = base.pre_translate(cx, cy).pre_scale(s, s);
            let r = rrect(-190.0, -112.0, 380.0, 224.0, 34.0);
            fill(&mut self.pm, &r, &paint_c(Color::from_rgba(14.0 / 255.0, 14.0 / 255.0, 28.0 / 255.0, 0.9 * a).unwrap()), card);
            self.pm.stroke_path(&r, &paint_c(colu(wc, 0.9 * a)), &Stroke { width: 3.0, ..Stroke::default() }, card, None);
            self.text("W I N N E R", cx, cy - 62.0, 20.0, [255, 211, 77], a, Align::Center, false);
            self.trophy(cx, cy - 6.0, a);
            self.text(&sc.chars[wi].name, cx, cy + 62.0, 54.0, [255, 255, 255], a, Align::Center, false);
        }
    }

    fn trophy(&mut self, cx: f32, cy: f32, a: f32) {
        let base = self.base();
        let gold = paint_c(Color::from_rgba(1.0, 0.83, 0.3, a).unwrap());
        let mut pb = PathBuilder::new();
        pb.move_to(cx - 18.0, cy - 26.0);
        pb.line_to(cx + 18.0, cy - 26.0);
        pb.cubic_to(cx + 18.0, cy - 4.0, cx + 8.0, cy + 4.0, cx, cy + 6.0);
        pb.cubic_to(cx - 8.0, cy + 4.0, cx - 18.0, cy - 4.0, cx - 18.0, cy - 26.0);
        pb.close();
        fill(&mut self.pm, &pb.finish().unwrap(), &gold, base);
        fill(&mut self.pm, &PathBuilder::from_rect(Rect::from_xywh(cx - 3.0, cy + 5.0, 6.0, 12.0).unwrap()), &gold, base);
        fill(&mut self.pm, &rrect(cx - 14.0, cy + 16.0, 28.0, 7.0, 3.0), &gold, base);
        let st = Stroke { width: 3.0, ..Stroke::default() };
        for sgn in [-1.0f32, 1.0] {
            let mut h = PathBuilder::new();
            h.move_to(cx + sgn * 18.0, cy - 22.0);
            h.cubic_to(cx + sgn * 30.0, cy - 22.0, cx + sgn * 30.0, cy - 6.0, cx + sgn * 14.0, cy - 4.0);
            self.pm.stroke_path(&h.finish().unwrap(), &gold, &st, base, None);
        }
    }

    fn confetti(&mut self, t0: f64, t: f64, sc: &Scene) {
        let a0 = t - t0;
        if !(0.0..=4.5).contains(&a0) { return; }
        let base = self.base();
        let mut cols: Vec<[u8; 3]> = sc.chars.iter().map(|c| c.color).collect();
        cols.push([255, 211, 77]);
        cols.push([255, 255, 255]);
        for i in 0..110 {
            let fi = i as f64;
            let ang = -std::f64::consts::FRAC_PI_2 + (h01(fi, 11.0) - 0.5) * 2.4;
            let sp = 260.0 + h01(fi, 12.0) * 520.0;
            let ph = h01(fi, 13.0) * 6.28;
            let a = a0 - h01(fi, 14.0) * 0.18;
            if a < 0.0 { continue; }
            let x = W / 2.0 + ang.cos() * sp * a * 0.62 + (a * 5.0 + ph).sin() * 14.0;
            let y = H * 0.62 + ang.sin() * sp * a + 0.5 * 620.0 * a * a;
            let rot = (ph + a * (4.0 + h01(fi, 15.0) * 8.0)).to_degrees() as f32;
            let alpha = ((4.5 - a0) / 1.2).clamp(0.0, 1.0) as f32;
            let hh = 12.0 * (0.4 + 0.6 * (a * 9.0 + ph).sin().abs()) as f32;
            let tf = base.pre_translate(x as f32, y as f32).pre_rotate(rot);
            fill(&mut self.pm, &PathBuilder::from_rect(Rect::from_xywh(-4.0, -6.0, 8.0, hh).unwrap()), &paint_c(colu(cols[i % cols.len()], alpha)), tf);
        }
    }

    fn title_card(&mut self, t: f64, title: &str) {
        if title.is_empty() || t > 2.6 { return; }
        let a = ((t / 0.35).min(1.0) * ((2.6 - t) / 0.5).min(1.0)).max(0.0) as f32;
        if a <= 0.0 { return; }
        let y = H as f32 * 0.3 + (1.0 - (t / 0.5).min(1.0) as f32) * 18.0;
        let base = self.base();
        fill(&mut self.pm, &PathBuilder::from_rect(Rect::from_xywh(0.0, y - 70.0, W as f32, 120.0).unwrap()), &paint_c(Color::from_rgba(0.0, 0.0, 0.0, 0.35 * a).unwrap()), base);
        self.text(&title.to_uppercase(), W as f32 / 2.0, y, 46.0, [255, 255, 255], a, Align::Center, true);
    }

    /// bloom + colour grade (+ film grain): the final-render finish
    fn post(&mut self) {
        let (w, h) = (self.w as usize, self.h as usize);
        // bloom: a blurred, quarter-size copy added back on top
        let (sw, sh) = ((w / 4).max(1), (h / 4).max(1));
        let mut small = vec![0f32; sw * sh * 3];
        let d = self.pm.data();
        for y in 0..sh { for x in 0..sw {
            let mut acc = [0f32; 3];
            for dy in 0..4 { for dx in 0..4 {
                let i = ((y * 4 + dy).min(h - 1) * w + (x * 4 + dx).min(w - 1)) * 4;
                for c in 0..3 { acc[c] += d[i + c] as f32; }
            } }
            for c in 0..3 { small[(y * sw + x) * 3 + c] = acc[c] / 16.0; }
        } }
        for _ in 0..3 { small = box_blur(&small, sw, sh, 3); }
        let d = self.pm.data_mut();
        for y in 0..h { for x in 0..w {
            let (fx, fy) = (((x as f32 + 0.5) / 4.0 - 0.5).clamp(0.0, sw as f32 - 1.001), ((y as f32 + 0.5) / 4.0 - 0.5).clamp(0.0, sh as f32 - 1.001));
            let (x0, y0) = (fx as usize, fy as usize);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            let i = (y * w + x) * 4;
            let gi = ((self.frame_no as usize * 37 + x) % 256 + ((y + self.frame_no as usize * 91) % 256) * 256) % self.grain.len();
            let g = (self.grain[gi] as f32 - 127.5) * 0.015;
            let mut px = [0f32; 3];
            for c in 0..3 {
                let s00 = small[(y0 * sw + x0) * 3 + c];
                let s10 = small[(y0 * sw + x0 + 1) * 3 + c];
                let s01 = small[((y0 + 1) * sw + x0) * 3 + c];
                let s11 = small[((y0 + 1) * sw + x0 + 1) * 3 + c];
                let bl = (s00 * (1.0 - tx) + s10 * tx) * (1.0 - ty) + (s01 * (1.0 - tx) + s11 * tx) * ty;
                px[c] = d[i + c] as f32 + bl * 0.36;
            }
            // colour grade: saturation 1.16, contrast 1.08, brightness 1.02
            let lum = 0.299 * px[0] + 0.587 * px[1] + 0.114 * px[2];
            for c in 0..3 {
                let v = lum + (px[c] - lum) * 1.16;
                let v = ((v / 255.0 - 0.5) * 1.08 + 0.5) * 1.02 * 255.0 + g;
                d[i + c] = v.clamp(0.0, 255.0) as u8;
            }
        } }
    }
}

fn box_blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0f32; src.len()];
    let mut out = vec![0f32; src.len()];
    let k = (2 * r + 1) as f32;
    for y in 0..h { for c in 0..3 {
        let mut acc = 0.0;
        for x in 0..=r.min(w - 1) { acc += src[(y * w + x) * 3 + c]; }
        acc += src[(y * w) * 3 + c] * r as f32; // clamp left edge
        for x in 0..w {
            tmp[(y * w + x) * 3 + c] = acc / k;
            let add = src[(y * w + (x + r + 1).min(w - 1)) * 3 + c];
            let sub = src[(y * w + x.saturating_sub(r)) * 3 + c];
            acc += add - sub;
        }
    } }
    for x in 0..w { for c in 0..3 {
        let mut acc = 0.0;
        for y in 0..=r.min(h - 1) { acc += tmp[(y * w + x) * 3 + c]; }
        acc += tmp[x * 3 + c] * r as f32;
        for y in 0..h {
            out[(y * w + x) * 3 + c] = acc / k;
            let add = tmp[((y + r + 1).min(h - 1) * w + x) * 3 + c];
            let sub = tmp[(y.saturating_sub(r) * w + x) * 3 + c];
            acc += add - sub;
        }
    } }
    out
}
