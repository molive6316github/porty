//! Maps and course generation: a faithful port of the web version, so the same seed gives the same course.
use crate::rng::{hash_seed, Rng};

pub const W: f64 = 540.0;
pub const H: f64 = 960.0;
pub const RAD: f64 = 13.0;
pub const BAND: f64 = 120.0;
pub const PAD: f64 = 45.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Ramp,
    Peg,
    Bump,
}

#[derive(Clone, Debug)]
pub struct Seg {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub r: f64,
    pub e: f64,
    pub kind: Kind,
    /// physical lifetime in sim seconds (bumpers composed from the music exist only briefly); vis0 = when its approach ring starts
    pub t0: f64,
    pub vis0: f64,
    pub t1: f64,
}

#[derive(Clone, Debug)]
pub struct Spinner {
    pub cx: f64,
    pub cy: f64,
    pub len: f64,
    pub w: f64,
    pub ph: f64,
    pub r: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg1: [f32; 3],
    pub bg2: [f32; 3],
    pub hb: f32,
    pub hs: f32,
    pub sp: [f32; 3],
}

pub struct MapDef {
    pub key: &'static str,
    pub label: &'static str,
    pub weights: Option<[f64; 5]>,
    pub pal: Option<Palette>,
}

fn hex(c: u32) -> [f32; 3] {
    [((c >> 16) & 255) as f32, ((c >> 8) & 255) as f32, (c & 255) as f32]
}

pub fn hsl(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 360.0;
    let (s, l) = (s / 100.0, l / 100.0);
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        if t < 0.0 {
            t += 1.0
        }
        if t > 1.0 {
            t -= 1.0
        }
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        v * 255.0
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

pub const MAPS: [MapDef; 6] = [
    MapDef { key: "neon", label: "Neon Mix · a bit of everything", weights: Some([1.0, 1.0, 1.0, 1.0, 1.0]),
        pal: Some(Palette { bg1: [13.0, 13.0, 28.0], bg2: [21.0, 10.0, 28.0], hb: 170.0, hs: 190.0, sp: [255.0, 157.0, 61.0] }) },
    MapDef { key: "pachinko", label: "Pachinko Palace · pegs & bumpers", weights: Some([0.3, 4.0, 1.0, 0.5, 2.0]),
        pal: Some(Palette { bg1: [42.0, 15.0, 46.0], bg2: [59.0, 19.0, 70.0], hb: 290.0, hs: 90.0, sp: [255.0, 211.0, 77.0] }) },
    MapDef { key: "slide", label: "Slide City · zig-zag ramps", weights: Some([4.0, 0.5, 1.0, 0.3, 0.8]),
        pal: Some(Palette { bg1: [7.0, 24.0, 43.0], bg2: [11.0, 44.0, 71.0], hb: 165.0, hs: 55.0, sp: [125.0, 249.0, 255.0] }) },
    MapDef { key: "spinner", label: "Spinner Gauntlet · lava", weights: Some([0.4, 1.0, 0.4, 4.0, 1.2]),
        pal: Some(Palette { bg1: [28.0, 8.0, 5.0], bg2: [51.0, 16.0, 10.0], hb: 0.0, hs: 55.0, sp: [255.0, 225.0, 77.0] }) },
    MapDef { key: "funnel", label: "Funnel Falls · bottlenecks", weights: Some([0.8, 1.5, 3.5, 0.6, 1.0]),
        pal: Some(Palette { bg1: [4.0, 20.0, 42.0], bg2: [7.0, 56.0, 79.0], hb: 185.0, hs: 60.0, sp: [255.0, 122.0, 217.0] }) },
    MapDef { key: "chaos", label: "Chaos · random map & colours", weights: None, pal: None },
];

pub fn map_index(key: &str) -> usize {
    MAPS.iter().position(|m| m.key == key).unwrap_or(0)
}

pub fn palette_for(key: &str, seed: &str) -> Palette {
    let m = &MAPS[map_index(key)];
    if let Some(p) = m.pal {
        return p;
    }
    let mut r = Rng::new(hash_seed(&format!("{seed}|pal")));
    let hb = (r.next() * 360.0).floor() as f32;
    let hs = 60.0 + r.next() as f32 * 160.0;
    Palette { bg1: hsl(hb, 45.0, 7.0), bg2: hsl(hb + 40.0, 50.0, 12.0), hb, hs, sp: hsl(hb + 180.0, 95.0, 62.0) }
}

#[derive(Clone)]
pub struct Course {
    pub segs: Vec<Seg>,
    pub spins: Vec<Spinner>,
    pub bands: Vec<Vec<u32>>,
    pub finish_y: f64,
    pub floor_y: f64,
    pub hc: f64,
    pub pal: Palette,
    pub map_key: &'static str,
}

pub fn build_course(hc: f64, seed: &str, map_key: &str) -> Course {
    let mi = map_index(map_key);
    let map = &MAPS[mi];
    let mut r = Rng::new(hash_seed(&format!("{seed}|course|{}", map.key)));
    let mut segs: Vec<Seg> = Vec::new();
    let mut spins: Vec<Spinner> = Vec::new();
    let mut seg = |x1: f64, y1: f64, x2: f64, y2: f64, rr: f64, e: f64, kind: Kind| segs.push(Seg { x1, y1, x2, y2, r: rr, e, kind, t0: -1e9, vis0: -1e9, t1: 1e9 });
    let mut y = 230.0;
    while y < hc - 150.0 {
        // choose a section type by the map's weights (chaos draws fresh weights every time, like the web version)
        let w: [f64; 5] = match map.weights {
            Some(w) => w,
            None => [r.next() * 3.0, r.next() * 3.0, r.next() * 3.0, r.next() * 3.0, r.next() * 3.0],
        };
        let tot: f64 = w.iter().sum();
        let mut rv = r.next() * tot;
        let mut t = 0usize;
        for (i, wi) in w.iter().enumerate() {
            rv -= wi;
            if rv <= 0.0 {
                t = i;
                break;
            }
        }
        match t {
            0 => {
                let cnt = 3 + (r.next() * 3.0).floor() as i32;
                let mut side = if r.next() < 0.5 { 0 } else { 1 };
                for _ in 0..cnt {
                    let gap = 62.0 + r.next() * 14.0;
                    let drop = 75.0 + r.next() * 35.0;
                    if side == 0 {
                        seg(0.0, y, W - gap, y + drop, 5.0, 0.35, Kind::Ramp);
                    } else {
                        seg(W, y, gap, y + drop, 5.0, 0.35, Kind::Ramp);
                    }
                    side ^= 1;
                    y += drop + 78.0; // the next ramp must clear the gap by more than a marble's width
                }
                y += 50.0;
            }
            1 => {
                let rows = 5 + (r.next() * 3.0).floor() as i32;
                for rr in 0..rows {
                    let off = (rr % 2) as f64 * 30.0;
                    let mut x = 60.0 + off;
                    while x <= W - 60.0 - off {
                        seg(x, y, x, y, 7.0, 0.5, Kind::Peg); // pegs stay clear of the walls so marbles can't wedge
                        x += 60.0;
                    }
                    y += 52.0;
                }
                y += 60.0;
            }
            2 => {
                let gap = 56.0 + r.next() * 20.0;
                seg(0.0, y, W / 2.0 - gap / 2.0, y + 110.0, 5.0, 0.3, Kind::Ramp);
                seg(W, y, W / 2.0 + gap / 2.0, y + 110.0, 5.0, 0.3, Kind::Ramp);
                y += 110.0;
                seg(W / 2.0 - 70.0, y + 80.0, W / 2.0 - 70.0, y + 80.0, 12.0, 0.7, Kind::Bump);
                seg(W / 2.0 + 70.0, y + 80.0, W / 2.0 + 70.0, y + 80.0, 12.0, 0.7, Kind::Bump);
                y += 200.0;
            }
            3 => {
                let dir = if r.next() < 0.5 { 1.0 } else { -1.0 };
                spins.push(Spinner { cx: W * 0.27, cy: y + 70.0, len: 92.0, w: 2.3 * dir, ph: r.next() * 6.28, r: 6.0 });
                spins.push(Spinner { cx: W * 0.73, cy: y + 70.0, len: 92.0, w: -2.3 * dir, ph: r.next() * 6.28, r: 6.0 });
                spins.push(Spinner { cx: W * 0.5, cy: y + 190.0, len: 100.0, w: 1.9 * dir, ph: r.next() * 6.28, r: 6.0 });
                y += 300.0;
            }
            _ => {
                let cnt = 4 + (r.next() * 2.0).floor() as i32;
                for i in 0..cnt {
                    // the web version drew two x values here and collided with a long invisible bar;
                    // the native version keeps the same random draws but makes each bumper a real round bumper
                    let x = 55.0 + r.next() * (W - 110.0);
                    let _unused = r.next();
                    seg(x, y + i as f64 * 68.0, x, y + i as f64 * 68.0, 22.0, 0.92, Kind::Bump);
                }
                y += cnt as f64 * 68.0 + 40.0;
            }
        }
        y += 50.0;
    }
    let finish_y = y + 60.0;
    let floor_y = finish_y + 170.0;
    segs.push(Seg { x1: 0.0, y1: floor_y, x2: W, y2: floor_y, r: 6.0, e: 0.2, kind: Kind::Ramp, t0: -1e9, vis0: -1e9, t1: 1e9 });
    let nb = (floor_y / BAND).ceil() as usize + 3;
    let mut bands: Vec<Vec<u32>> = vec![Vec::new(); nb];
    for (i, s) in segs.iter().enumerate() {
        let lo = (((s.y1.min(s.y2) - s.r - PAD) / BAND).floor() as i64).max(0) as usize;
        let hi = (((s.y1.max(s.y2) + s.r + PAD) / BAND).floor() as i64).min(nb as i64 - 1).max(0) as usize;
        for b in lo..=hi {
            bands[b].push(i as u32);
        }
    }
    Course { segs, spins, bands, finish_y, floor_y, hc, pal: palette_for(map.key, seed), map_key: map.key }
}

impl Course {
    /// A bumper that exists only from `t0` to `t1` (sim seconds); its approach ring is drawn from `vis0`.
    pub fn add_timed_bumper(&mut self, x: f64, y: f64, r: f64, vis0: f64, t0: f64, t1: f64) {
        let i = self.segs.len() as u32;
        self.segs.push(Seg { x1: x, y1: y, x2: x, y2: y, r, e: 1.12, kind: Kind::Bump, t0, vis0, t1 });
        let nb = self.bands.len();
        let lo = (((y - r - PAD) / BAND).floor() as i64).max(0) as usize;
        let hi = (((y + r + PAD) / BAND).floor() as i64).min(nb as i64 - 1).max(0) as usize;
        for b in lo..=hi { self.bands[b].push(i); }
    }
}
