//! Marble physics: a port of the web version's simulation (same constants, same order of operations).
use crate::course::*;
use crate::rng::{hash_seed, Rng};

pub const FPS: f64 = 60.0;
pub const GRAV: f64 = 1000.0;
pub const DT: f64 = 1.0 / 120.0;

#[derive(Clone, Copy)]
struct Marble {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    fin: f64,
    still: i32,
    last_ev: i64,
    last_mm: i64,
    contact: i64, // last frame this marble touched the course
}

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub seg: u32, // which course segment was hit (u32::MAX = a marble)
    pub m: u8,
    pub f: u32,
    pub kind: u8, // 0 peg, 1 bumper, 2 ramp, 3 marble-on-marble
    pub v: f32,
    pub x: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct SchedKey {
    pub ts: f64,
    pub m: usize,
}

pub struct Sim {
    pub n: usize,
    pub frames: usize,
    pub pos: Vec<f32>, // frames * n * 2
    pub lead: Vec<u8>,
    pub cam: Vec<f32>,
    pub twin: f64,
    pub winner: usize,
    pub fins: Vec<f64>,
    pub events: Vec<Event>,
    pub touch: Vec<u8>, // frames * n: 1 while that marble is touching the course
    /// optional ranking key per frame and marble (higher = further ahead); empty means rank by height
    pub rank: Vec<f32>,
}

impl Sim {
    pub fn at(&self, f: usize, i: usize) -> (f64, f64) {
        let k = (f * self.n + i) * 2;
        (self.pos[k] as f64, self.pos[k + 1] as f64)
    }
}

fn hit_seg(m: &mut Marble, x1: f64, y1: f64, x2: f64, y2: f64, r: f64, e: f64, kick: f64) -> f64 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let l2 = dx * dx + dy * dy;
    let mut u = if l2 > 0.0 { ((m.x - x1) * dx + (m.y - y1) * dy) / l2 } else { 0.0 };
    u = u.clamp(0.0, 1.0);
    let (cx, cy) = (x1 + dx * u, y1 + dy * u);
    let (mut nx, mut ny) = (m.x - cx, m.y - cy);
    let rr = RAD + r;
    let d2 = nx * nx + ny * ny;
    if d2 >= rr * rr {
        return 0.0;
    }
    let d = d2.sqrt();
    if d < 1e-6 {
        nx = 0.0;
        ny = -1.0;
    } else {
        nx /= d;
        ny /= d;
    }
    m.x = cx + nx * rr;
    m.y = cy + ny * rr;
    let vn = m.vx * nx + m.vy * ny;
    if vn < 0.0 {
        m.vx -= (1.0 + e) * vn * nx;
        m.vy -= (1.0 + e) * vn * ny;
        m.vx *= 0.998;
        if kick > 0.0 { let out = m.vx * nx + m.vy * ny; if out < kick { m.vx += (kick - out) * nx; m.vy += (kick - out) * ny; } }
        return -vn;
    }
    0.0
}

/// The physics as a steppable object, so a composer can look at the race at any moment and add things to the course as it goes.
pub struct Engine {
    pub n: usize,
    ms: Vec<Marble>,
    pub f: usize,
    cam_y: f64,
    rn: Rng,
    di: i64,
    winner: i64,
    done: usize,
    end_f: i64,
    win_f: i64,
    pub ended: bool,
    /// per-marble rolling friction (applied only while touching the course, so flights stay exactly ballistic)
    pub roll_drag: Vec<f64>,
    /// a ghost marble passes through the others (the composer uses this so a planned landing cannot be knocked off course)
    pub ghost: Vec<bool>,
    pos: Vec<f32>,
    lead: Vec<u8>,
    cam: Vec<f32>,
    touch: Vec<u8>,
    events: Vec<Event>,
}

/// What the composer needs to know about a marble right now.
#[derive(Clone, Copy, Debug)]
pub struct Look { pub x: f64, pub y: f64, pub vx: f64, pub vy: f64, pub leader: usize }

impl Engine {
    pub fn new(n: usize, nudge_seed: u32, seed: &str) -> Engine {
        let mut r0 = Rng::new(hash_seed(&format!("{seed}|start")));
        let mut ms: Vec<Marble> = Vec::with_capacity(n);
        for i in 0..n {
            let (row, col, per) = (i % 2, i / 2, (n + 1) / 2);
            let x = 70.0 + (W - 140.0) * ((col as f64 + if row == 1 { 0.5 } else { 0.0 } + r0.next() * 0.2) / (per as f64 - 0.5).max(1.0));
            let vx = (r0.next() - 0.5) * 30.0;
            ms.push(Marble { x, y: 50.0 + row as f64 * 36.0, vx, vy: 0.0, fin: -1.0, still: 0, last_ev: -9, last_mm: -9, contact: -99 });
        }
        Engine { n, ms, f: 0, cam_y: 300.0, rn: Rng::new(nudge_seed), di: -1, winner: -1, done: 0, end_f: -1, win_f: -1, ended: false, roll_drag: vec![0.0; n], ghost: vec![false; n], pos: vec![], lead: vec![], cam: vec![], touch: vec![], events: vec![] }
    }
    pub fn time(&self) -> f64 { self.f as f64 / FPS }
    pub fn state(&self, i: usize) -> (f64, f64, f64, f64, bool) { let m = &self.ms[i]; (m.x, m.y, m.vx, m.vy, m.fin >= 0.0) }
    pub fn touching(&self, i: usize) -> bool { self.f as i64 - self.ms[i].contact <= 2 }
    /// the leading marble and where it is going
    pub fn look(&self) -> Look {
        let mut l = 0usize;
        let mut best = f64::NEG_INFINITY;
        for (i, m) in self.ms.iter().enumerate() { let sc = if m.fin >= 0.0 { 1e9 - m.fin } else { m.y }; if sc > best { best = sc; l = i; } }
        let m = self.ms[l];
        Look { x: m.x, y: m.y, vx: m.vx, vy: m.vy, leader: l }
    }
    /// Advance one 60 Hz frame. Returns false once the race is over.
    pub fn step(&mut self, course: &Course, sched: Option<&[SchedKey]>) -> bool {
        if self.ended { return false; }
        let n = self.n;
        let f = self.f;
        let t = f as f64 / FPS;
        for i in 0..n {
            self.pos.push(self.ms[i].x as f32);
            self.pos.push(self.ms[i].y as f32);
            self.touch.push((f as i64 - self.ms[i].contact <= 2) as u8);
        }
        let lk = self.look();
        self.lead.push(lk.leader as u8);
        let finish_y = course.finish_y;
        let ly = self.ms.iter().map(|m| m.y).fold(f64::NEG_INFINITY, f64::max);
        self.cam_y += (ly.min(finish_y - 80.0) - self.cam_y) * 0.1;
        self.cam.push(self.cam_y as f32);
        if self.win_f >= 0 && self.end_f < 0 && f as i64 >= self.win_f + (3.0 * FPS).floor() as i64 { self.end_f = f as i64; }
        if self.done == n && self.end_f < 0 { self.end_f = f as i64 + FPS as i64; }
        if self.end_f >= 0 && f as i64 >= self.end_f { self.ended = true; self.f += 1; return false; }
        for s in 0..2 {
            let st = t + s as f64 * DT;
            if let Some(sc) = sched {
                // the director: nudge the marble that should lead forward, slow the current leader. It only ever adds a little
                // downhill pull or drag, never an upward force, so nothing floats.
                while ((self.di + 1) as usize) < sc.len() && sc[(self.di + 1) as usize].ts <= st { self.di += 1; }
                if self.di >= 0 {
                    let d = sc[self.di as usize].m;
                    let (mut lm, mut ly2) = (-1i64, f64::NEG_INFINITY);
                    for q in 0..n { if self.ms[q].fin < 0.0 && self.ms[q].y > ly2 { ly2 = self.ms[q].y; lm = q as i64; } }
                    if lm >= 0 && self.ms[d].fin < 0.0 {
                        if lm as usize != d {
                            let gap = ly2 - self.ms[d].y;
                            let k = (gap / 90.0).clamp(0.0, 1.0);
                            self.ms[d].vy += GRAV * DT * (0.12 + 0.28 * k);
                            let lmi = lm as usize;
                            let drag = 0.004 + 0.012 * k;
                            self.ms[lmi].vy *= 1.0 - drag;
                            self.ms[lmi].vx *= 1.0 - drag * 0.5;
                        } else {
                            self.ms[d].vy += GRAV * DT * 0.12;
                            let dy = self.ms[d].y;
                            for q in 0..n { if q != d && self.ms[q].fin < 0.0 && dy - self.ms[q].y > 220.0 { self.ms[q].vy += GRAV * DT * 0.25; } }
                        }
                    }
                }
            }
            for mi in 0..n {
                let mut m = self.ms[mi];
                m.vy += GRAV * DT;
                let sp = m.vx.hypot(m.vy);
                if sp > 1300.0 { m.vx *= 1300.0 / sp; m.vy *= 1300.0 / sp; }
                m.x += m.vx * DT;
                m.y += m.vy * DT;
                if m.x < RAD { m.x = RAD; m.vx = m.vx.abs() * 0.5; } else if m.x > W - RAD { m.x = W - RAD; m.vx = -m.vx.abs() * 0.5; }
                let bidx = ((m.y / BAND).floor() as i64).clamp(0, course.bands.len() as i64 - 1) as usize;
                let (cx0, cy0) = (m.x, m.y);
                for &si in &course.bands[bidx] {
                    let sg = &course.segs[si as usize];
                    if st < sg.t0 || st > sg.t1 { continue; } // timed bumpers only exist for a moment
                    let timed = sg.t0 > -1e8;
                    let mut imp = hit_seg(&mut m, sg.x1, sg.y1, sg.x2, sg.y2, sg.r, sg.e, if timed { 320.0 } else { 0.0 });
                    if sg.kick > 0.0 && imp > 0.0 { m.vy = -sg.kick; m.vx = sg.bvx; imp = imp.max(200.0); } // trampoline: an exact launch
                    if imp > if timed { 25.0 } else { 140.0 } && f as i64 - m.last_ev > 2 {
                        m.last_ev = f as i64;
                        let kind = match sg.kind { Kind::Peg => 0, Kind::Bump => 1, Kind::Ramp => 2 };
                        self.events.push(Event { seg: si, m: mi as u8, f: f as u32, kind, v: imp as f32, x: m.x as f32 });
                    }
                }
                if m.x != cx0 || m.y != cy0 { m.contact = f as i64; }
                if f as i64 - m.contact <= 1 && self.roll_drag[mi] != 0.0 { m.vx *= 1.0 - self.roll_drag[mi]; m.vy *= 1.0 - self.roll_drag[mi]; }
                for sp2 in &course.spins {
                    if (m.y - sp2.cy).abs() < sp2.len + 30.0 {
                        let a = sp2.ph + sp2.w * st;
                        let (c, sn) = (a.cos() * sp2.len, a.sin() * sp2.len);
                        let (px, py) = (m.x, m.y);
                        hit_seg(&mut m, sp2.cx - c, sp2.cy - sn, sp2.cx + c, sp2.cy + sn, sp2.r, 0.5, 0.0);
                        if m.x != px || m.y != py {
                            let (rx, ry) = (m.x - sp2.cx, m.y - sp2.cy);
                            m.vx += -ry * sp2.w * 0.35 * DT * 60.0 * 0.1;
                            m.vy += rx * sp2.w * 0.35 * DT * 60.0 * 0.1;
                        }
                    }
                }
                if m.vx.hypot(m.vy) < 12.0 && m.y < finish_y {
                    m.still += 1;
                    if m.still > 150 { m.vx += (if m.x < W / 2.0 { 1.0 } else { -1.0 }) * (220.0 + self.rn.next() * 160.0); m.vy -= 160.0; m.still = 0; }
                } else { m.still = 0; }
                if m.fin < 0.0 && m.y > finish_y {
                    m.fin = st;
                    self.done += 1;
                    if self.winner < 0 { self.winner = mi as i64; self.win_f = f as i64; }
                }
                self.ms[mi] = m;
            }
            for i in 0..n {
                for j in (i + 1)..n {
                    if self.ghost[i] || self.ghost[j] { continue; }
                    let (a, b) = (self.ms[i], self.ms[j]);
                    let (mut dx, mut dy) = (b.x - a.x, b.y - a.y);
                    let d2 = dx * dx + dy * dy;
                    let rr = RAD * 2.0;
                    if d2 < rr * rr && d2 > 1e-9 {
                        let d = d2.sqrt();
                        dx /= d; dy /= d;
                        let ov = (rr - d) / 2.0;
                        let (mut a, mut b) = (a, b);
                        a.x -= dx * ov; a.y -= dy * ov; b.x += dx * ov; b.y += dy * ov;
                        let vn = (b.vx - a.vx) * dx + (b.vy - a.vy) * dy;
                        if vn < 0.0 {
                            let j2 = -(1.0 + 0.6) * vn / 2.0;
                            a.vx -= j2 * dx; a.vy -= j2 * dy; b.vx += j2 * dx; b.vy += j2 * dy;
                            if -vn > 160.0 && f as i64 - a.last_mm > 3 {
                                a.last_mm = f as i64;
                                self.events.push(Event { seg: u32::MAX, m: i as u8, f: f as u32, kind: 3, v: (-vn) as f32, x: ((a.x + b.x) / 2.0) as f32 });
                            }
                        }
                        self.ms[i] = a; self.ms[j] = b;
                    }
                }
            }
        }
        self.f += 1;
        true
    }
    pub fn finish(self, max_t: f64) -> Sim {
        let n = self.n;
        let mut winner = self.winner;
        if winner < 0 { let mut b = 0; for i in 1..n { if self.ms[i].y > self.ms[b].y { b = i; } } winner = b as i64; }
        let w = winner as usize;
        let twin = if self.ms[w].fin >= 0.0 { self.ms[w].fin } else { max_t };
        let frames = self.lead.len();
        Sim { n, frames, pos: self.pos, lead: self.lead, cam: self.cam, twin, winner: w, fins: self.ms.iter().map(|m| m.fin).collect(), events: self.events, touch: self.touch, rank: vec![] }
    }
}

pub fn simulate(course: &Course, n: usize, _beats: &[f64], nudge_seed: u32, seed: &str, max_t: f64, sched: Option<&[SchedKey]>) -> Sim {
    let mut e = Engine::new(n, nudge_seed, seed);
    let max_f = (max_t * FPS).ceil() as usize + 1;
    while e.f < max_f && e.step(course, sched) {}
    e.finish(max_t)
}

/// Maps song time to simulation time. A straight line at first; snapping bends it a little so impacts land on the beat.
#[derive(Clone, Debug)]
pub struct Warp { pub pts: Vec<(f64, f64)> } // (song seconds, sim seconds), both strictly increasing

impl Warp {
    pub fn linear(factor: f64) -> Warp { Warp { pts: vec![(0.0, 0.0), (10_000.0, 10_000.0 * factor)] } }
    fn interp(pts: &[(f64, f64)], x: f64, rev: bool) -> f64 {
        let (a, b): (Vec<f64>, Vec<f64>) = if rev { (pts.iter().map(|p| p.1).collect(), pts.iter().map(|p| p.0).collect()) } else { (pts.iter().map(|p| p.0).collect(), pts.iter().map(|p| p.1).collect()) };
        let n = a.len();
        let j = a.partition_point(|v| *v <= x).clamp(1, n - 1);
        let (x0, x1, y0, y1) = (a[j - 1], a[j], b[j - 1], b[j]);
        y0 + (y1 - y0) * (x - x0) / (x1 - x0).max(1e-9)
    }
    pub fn to_sim(&self, song_t: f64) -> f64 { Self::interp(&self.pts, song_t, false) }
    pub fn to_song(&self, sim_t: f64) -> f64 { Self::interp(&self.pts, sim_t, true) }
}

/// Bend time so the strongest impacts (pegs, bumpers, ramps, marble hits) land exactly on a beat or half-beat.
/// For every half-beat cell the strongest impact is a candidate; a small dynamic program chooses which grid point each one
/// snaps to so playback never speeds up or slows down by more than about 25 %.
pub fn snap_warp(sim: &Sim, beats: &[f64], factor: f64) -> Warp {
    if beats.len() < 6 { return Warp::linear(factor); }
    let mut grid: Vec<f64> = vec![];
    for w in beats.windows(2) { grid.push(w[0]); grid.push((w[0] + w[1]) * 0.5); }
    grid.push(*beats.last().unwrap());
    // strongest impact per grid cell, in natural (unwarped) song time
    let mut cell: std::collections::BTreeMap<usize, Vec<(f32, f64, f64)>> = Default::default(); // v, sim s, song e
    for ev in &sim.events {
        if ev.v < 240.0 { continue; }
        let s = ev.f as f64 / FPS;
        let e = s / factor;
        if e < 0.8 || e < grid[0] || e > *grid.last().unwrap() { continue; }
        let gi = grid.partition_point(|g| *g <= e).saturating_sub(1);
        let v = ev.v * if ev.kind == 3 { 0.8 } else { 1.0 };
        let ent = cell.entry(gi).or_default();
        ent.push((v, s, e));
        ent.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        ent.truncate(2);
    }
    let mut evs: Vec<(f64, f64)> = cell.values().flat_map(|c| c.iter().map(|x| (x.2, x.1))).collect(); // (e, s)
    evs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    if evs.len() < 2 { return Warp::linear(factor); }
    // candidate grid points: the nearest few within a fifth of a second of the natural time
    let cands: Vec<Vec<f64>> = evs.iter().map(|(e, _)| {
        let j = grid.partition_point(|g| *g <= *e);
        let mut c: Vec<f64> = grid[j.saturating_sub(2)..(j + 2).min(grid.len())].iter().cloned().filter(|g| (g - e).abs() <= 0.2).collect();
        if c.is_empty() { c.push(grid[j.saturating_sub(1).min(grid.len() - 1)]); }
        c
    }).collect();
    let (lo_r, hi_r) = (0.8, 1.25);
    let ok = |c0: f64, s0: f64, c1: f64, s1: f64| -> bool { let dc = c1 - c0; dc > 0.05 && { let r = ((s1 - s0) / dc) / factor; r >= lo_r && r <= hi_r } };
    let skip_cost = 0.0025;
    let m = evs.len();
    let maxskip = 4usize;
    // dp over chosen impacts: node (i, k) = impact i snapped to its k-th candidate; edges may skip a few impacts at a cost
    let inf = f64::MAX / 4.0;
    let mut dp: Vec<Vec<(f64, i64, usize)>> = vec![]; // cost, prev impact (-1 = start), prev candidate
    for i in 0..m {
        let mut row = vec![];
        for c in &cands[i] {
            let own = (c - evs[i].0).powi(2);
            let mut best = (inf, -2i64, 0usize);
            if i <= maxskip && ok(0.0, 0.0, *c, evs[i].1) { best = (skip_cost * i as f64 + own, -1, 0); }
            for j in i.saturating_sub(maxskip + 1)..i {
                for (kj, cj) in cands[j].iter().enumerate() {
                    if dp[j][kj].0 >= inf { continue; }
                    if ok(*cj, evs[j].1, *c, evs[i].1) {
                        let v = dp[j][kj].0 + skip_cost * (i - j - 1) as f64 + own;
                        if v < best.0 { best = (v, j as i64, kj); }
                    }
                }
            }
            row.push(best);
        }
        dp.push(row);
    }
    let end = (sim.twin / factor, sim.twin);
    // finish on the natural mapping if the speed limit allows it, otherwise just take the best chain
    let mut fin: Option<(f64, usize, usize)> = None;
    for i in m.saturating_sub(maxskip + 1)..m {
        for (k, c) in cands[i].iter().enumerate() {
            if dp[i][k].0 >= inf { continue; }
            let reach = ok(*c, evs[i].1, end.0, end.1);
            let v = dp[i][k].0 + skip_cost * (m - 1 - i) as f64 + if reach { 0.0 } else { 5.0 };
            if fin.map(|f| v < f.0).unwrap_or(true) { fin = Some((v, i, k)); }
        }
    }
    let Some((_, mut i, mut k)) = fin else { return Warp::linear(factor) };
    let mut chosen: Vec<(f64, f64)> = vec![]; // (song c, sim s)
    loop {
        chosen.push((cands[i][k], evs[i].1));
        let (_, pj, pk) = dp[i][k];
        if pj < 0 { break; }
        i = pj as usize; k = pk;
    }
    chosen.reverse();
    let mut pts: Vec<(f64, f64)> = vec![(0.0, 0.0)];
    for (c, s) in chosen { let l = *pts.last().unwrap(); if c > l.0 + 0.02 && s > l.1 + 0.02 { pts.push((c, s)); } }
    let l = *pts.last().unwrap();
    if end.0 > l.0 + 0.05 && end.1 > l.1 + 0.05 { pts.push(end); }
    let l = *pts.last().unwrap();
    pts.push((l.0 + 10_000.0, l.1 + 10_000.0 * factor));
    Warp { pts }
}

/// A race laid out against the clock: the course is sized so the winner finishes about 2 s before the clip ends.
pub struct Timeline {
    pub course: Course,
    pub sim: Sim,
    pub factor: f64,
    pub frames: usize,
    pub lead_a: Vec<u8>,
    pub win_time: f64,
    pub n: usize,
    pub len: f64,
    pub landed: Option<(usize, usize)>,
    pub warp: Warp,
    /// the named marbles (the rest, in beat mode, are the crowd)
    pub nplayers: usize,
}

impl Timeline {
    /// simulation frame (fractional) shown at song time `t` seconds
    pub fn sim_frame(&self, t: f64) -> f64 {
        (self.warp.to_sim(t) * FPS).min(self.sim.frames as f64 - 1.0).max(0.0)
    }
}

pub struct RaceSetup<'a> {
    pub seed: &'a str,
    pub map: &'a str,
    pub n: usize,
    pub len: f64,
    pub beats: &'a [f64],
    /// who should lead from when (seconds into the clip): the director's plan
    pub plan: &'a [(f64, usize)],
    pub winner: Option<usize>,
    /// (bar start, intensity 0..1) from the music, and whether to compose the course from the beats
    pub bars: &'a [(f64, f32)],
    pub compose: bool,
    pub crowd: usize,
    /// compose a designed map (ramps and trampolines) instead of the lane pads
    pub designed: bool,
}

pub fn build_timeline(s: &RaceSetup) -> Timeline {
    if s.designed && s.beats.len() > 8 {
        let cfg = crate::designed::DesignCfg { beats: s.beats, bars: s.bars, n: s.n, seed: s.seed, map: s.map, len: s.len, plan: s.plan, winner: s.winner };
        if let Some(d) = crate::designed::build(&cfg) {
            let frames = (s.len * FPS).ceil() as usize;
            let lead_a: Vec<u8> = (0..=frames).map(|f| d.sim.lead[f.min(d.sim.frames - 1)]).collect();
            let win_time = d.sim.twin;
            return Timeline { course: d.course, sim: d.sim, factor: 1.0, frames, lead_a, win_time, n: s.n, len: s.len, landed: None, warp: Warp::linear(1.0), nplayers: s.n };
        }
    }
    if s.compose && s.beats.len() > 8 {
        let cfg = crate::beatmode::BeatCfg { beats: s.beats, bars: s.bars, n: s.n, seed: s.seed, map: s.map, len: s.len, plan: s.plan, winner: s.winner, crowd: s.crowd };
        if let Some((course, sim, total)) = crate::beatmode::build(&cfg) {
            let frames = (s.len * FPS).ceil() as usize;
            let lead_a: Vec<u8> = (0..=frames).map(|f| sim.lead[f.min(sim.frames - 1)]).collect();
            let win_time = sim.twin;
            let _ = total;
            return Timeline { course, sim, factor: 1.0, frames, lead_a, win_time, n: s.n, len: s.len, landed: None, warp: Warp::linear(1.0), nplayers: s.n };
        }
    }
    let base = hash_seed(&format!("{}|n", s.seed));
    let (len, n) = (s.len, s.n);
    let mut hc = len * 210.0;
    let mut best: Option<(f64, Course, Sim)> = None;
    for _ in 0..7 {
        let course = build_course(hc, s.seed, s.map);
        let sim = simulate(&course, n, s.beats, base, s.seed, len * 3.0, None);
        let ratio = (len - 2.5) / sim.twin;
        let err = ratio.ln().abs();
        let better = best.as_ref().map(|b| err < b.0).unwrap_or(true);
        if better {
            best = Some((err, course, sim));
        }
        if ratio > 0.95 && ratio < 1.05 {
            break;
        }
        hc *= ratio.clamp(0.4, 2.2);
    }
    let (_, course, mut sim) = best.unwrap();
    let mut keys: Vec<(f64, usize)> = s.plan.iter().filter(|(t, m)| *m < n && *t >= 0.0 && *t < len - 3.0).cloned().collect();
    keys.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    if let Some(w) = s.winner.filter(|w| *w < n) {
        let last = keys.last().map(|k| k.0).unwrap_or(-1.0);
        keys.push(((len - 7.0).max(last + 1.5).min(len - 4.5), w));
    }
    let mut landed = None;
    if !keys.is_empty() {
        let mut factor = sim.twin / (len - 2.0).max(1.0);
        let mut best_sc: Option<(usize, Sim)> = None;
        let tries = 24 + 10 * keys.len().min(5) as u32; // more rigged moments need more attempts to line up
        for k in 0..tries {
            let sc: Vec<SchedKey> = keys.iter().map(|(t, m)| SchedKey { ts: t * factor - 2.4, m: *m }).collect();
            let s2 = simulate(&course, n, s.beats, base.wrapping_add(k.wrapping_mul(7919)), s.seed, len * 3.0, Some(&sc));
            let f2 = s2.twin / (len - 2.0).max(1.0);
            let score = keys.iter().filter(|(t, m)| {
                let fr = (((t + 0.4) * f2 * FPS).floor() as usize).min(s2.frames - 1);
                s2.lead[fr] as usize == *m
            }).count();
            if best_sc.as_ref().map(|b| score > b.0).unwrap_or(true) {
                best_sc = Some((score, s2));
            }
            factor = f2;
            if score == keys.len() {
                break;
            }
        }
        let (score, s2) = best_sc.unwrap();
        landed = Some((score, keys.len()));
        sim = s2;
    }
    let factor = sim.twin / (len - 2.0).max(1.0);
    let warp = snap_warp(&sim, s.beats, factor);
    let frames = (len * FPS).ceil() as usize;
    let lead_a: Vec<u8> = (0..=frames).map(|f| sim.lead[((warp.to_sim(f as f64 / FPS) * FPS).floor() as usize).min(sim.frames - 1)]).collect();
    let win_time = warp.to_song(sim.twin);
    Timeline { course, sim, factor, frames, lead_a, win_time, n, len, landed, warp, nplayers: n }
}

/// Marble indexes, first place to last, from what really happened in the physics.
pub fn finish_order(sim: &Sim) -> Vec<usize> {
    let last = sim.frames - 1;
    let mut idx: Vec<usize> = (0..sim.n).collect();
    idx.sort_by(|&a, &b| {
        let (fa, fb) = (sim.fins[a], sim.fins[b]);
        if fa >= 0.0 && fb >= 0.0 {
            fa.partial_cmp(&fb).unwrap()
        } else if fa >= 0.0 {
            std::cmp::Ordering::Less
        } else if fb >= 0.0 {
            std::cmp::Ordering::Greater
        } else {
            sim.at(last, b).1.partial_cmp(&sim.at(last, a).1).unwrap()
        }
    });
    idx
}

/// Fraction of beats that have a strong impact within 25 ms of them: (before warping, after warping)
pub fn beat_coverage(tl: &Timeline, beats: &[f64]) -> (f64, f64) {
    let ev: Vec<(f64, f64)> = tl.sim.events.iter().filter(|e| e.v >= 140.0).map(|e| { let s = e.f as f64 / FPS; (s / tl.factor, tl.warp.to_song(s)) }).collect();
    let bs: Vec<f64> = beats.iter().cloned().filter(|b| *b > 1.0 && *b < tl.win_time).collect();
    let cov = |sel: &dyn Fn(&(f64, f64)) -> f64| bs.iter().filter(|b| ev.iter().any(|e| (sel(e) - **b).abs() < 0.025)).count() as f64 / bs.len().max(1) as f64;
    (cov(&|e| e.0), cov(&|e| e.1))
}

/// How well the strong impacts sit on the beat grid: (impacts counted, within 25 ms before warping, within 25 ms after, slowest speed, fastest speed)
pub fn alignment_stats(tl: &Timeline, beats: &[f64]) -> (usize, usize, usize, f64, f64) {
    let mut grid: Vec<f64> = vec![];
    for w in beats.windows(2) { grid.push(w[0]); grid.push((w[0] + w[1]) * 0.5); }
    let near = |t: f64| grid.iter().map(|g| (g - t).abs()).fold(f64::MAX, f64::min);
    let (mut n, mut b, mut a) = (0, 0, 0);
    for ev in &tl.sim.events {
        if ev.v < 240.0 { continue; }
        let s = ev.f as f64 / FPS;
        let nat = s / tl.factor;
        if nat < 1.0 || nat > tl.win_time { continue; }
        n += 1;
        if near(nat) < 0.025 { b += 1; }
        if near(tl.warp.to_song(s)) < 0.025 { a += 1; }
    }
    let (mut lo, mut hi) = (f64::MAX, 0.0f64);
    for w in tl.warp.pts.windows(2) { if w[1].0 < tl.win_time + 1.0 && w[0].0 >= 0.0 { let sl = (w[1].1 - w[0].1) / (w[1].0 - w[0].0) / tl.factor; lo = lo.min(sl); hi = hi.max(sl); } }
    (n, b, a, lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn beat_mode_is_locked_to_the_music_but_varied() {
        let beats: Vec<f64> = (0..70).map(|i| 0.4 + i as f64 * 60.0 / 128.0).collect();
        let bars: Vec<(f64, f32)> = (0..18).map(|i| (0.4 + i as f64 * 4.0 * 60.0 / 128.0, [0.1f32, 0.2, 0.4, 0.6, 0.9, 1.0, 1.0, 0.9, 0.5, 0.3, 0.2, 0.2, 0.7, 0.9, 1.0, 0.8, 0.4, 0.2][i])).collect();
        let plan = [(8.0, 2usize), (16.0, 4usize)];
        let mut grid: Vec<f64> = vec![];
        for w in beats.windows(2) { grid.push(w[0]); grid.push((w[0] + w[1]) * 0.5); }
        for map in ["neon", "pachinko", "slide", "spinner", "funnel", "chaos"] {
            let tl = build_timeline(&RaceSetup { seed: "beat-test", map, n: 6, len: 30.0, beats: &beats, plan: &plan, winner: Some(3), bars: &bars, compose: true, crowd: 0, designed: false });
            let named: Vec<&Event> = tl.sim.events.iter().filter(|e| (e.m as usize) < 6).collect();
            // 1. every hit sits on the beat / half-beat grid
            let off_grid = named.iter().filter(|e| { let t = e.f as f64 / FPS; grid.iter().map(|g| (g - t).abs()).fold(f64::MAX, f64::min) > 1.0 / FPS + 1e-6 }).count();
            // 2. every beat has someone hitting, but rarely everyone at once
            let bs: Vec<f64> = beats.iter().cloned().filter(|b| *b > 2.5 && *b < tl.win_time - 1.0).collect();
            let who = |b: f64| (0..6).filter(|m| named.iter().any(|e| e.m as usize == *m && ((e.f as f64 / FPS) - b).abs() <= 1.0 / FPS + 1e-9)).count();
            let covered = bs.iter().filter(|b| who(**b) >= 1).count();
            let unison = bs.iter().filter(|b| who(**b) == 6).count() as f64 / bs.len() as f64;
            // 3. variety of pads, and orbs in the minority
            let pads: Vec<u8> = tl.course.segs.iter().filter(|s| s.owner < 6).map(|s| s.shape).collect();
            let orbs = pads.iter().filter(|s| **s == shape::ORB).count() as f64 / pads.len() as f64;
            let kinds: std::collections::BTreeSet<u8> = pads.iter().cloned().collect();
            // 4. no overlaps, sane speeds
            let mut min_gap = f64::MAX; let mut max_v = 0.0f64;
            for f in 1..tl.sim.frames { for a in 0..6 { let (ax, ay) = tl.sim.at(f, a); let (px, py) = tl.sim.at(f - 1, a); max_v = max_v.max((ax - px).hypot(ay - py) * FPS); for b in (a + 1)..6 { let (bx, by) = tl.sim.at(f, b); min_gap = min_gap.min((ax - bx).hypot(ay - by)); } } }
            // 5. not crowded: pads belonging to marbles that are on screen at the same time
            let mut seen = 0.0; let mut samples = 0.0;
            for f in (60..tl.sim.frames).step_by(30) { let cam = tl.sim.cam[f] as f64; seen += tl.course.segs.iter().filter(|g| g.owner < 6 && g.y1 > cam - 400.0 && g.y1 < cam + 560.0).count() as f64; samples += 1.0; }
            let visible_pads = seen / samples;
            // 6. a full map: decoration exists and never touches a marble
            let decor: Vec<&Seg> = tl.course.segs.iter().filter(|g| g.t0 > 1e8).collect();
            let mut worst = f64::MAX;
            for g in &decor { for f in (0..tl.sim.frames).step_by(2) { for m in 0..6 { let (px, py) = tl.sim.at(f, m);
                let (dx, dy) = (g.x2 - g.x1, g.y2 - g.y1); let l2 = dx * dx + dy * dy; let u = if l2 > 0.0 { (((px - g.x1) * dx + (py - g.y1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
                worst = worst.min((px - (g.x1 + dx * u)).hypot(py - (g.y1 + dy * u)) - g.r); } } }
            eprintln!("{map}: pads in view {:.1}, decoration {} pieces, closest marble to any decoration {:.0}px", visible_pads, decor.len(), worst);
            assert!(visible_pads <= 18.0, "{map}: too crowded ({visible_pads})");
            assert!(decor.len() >= 120, "{map}: not enough map ({})", decor.len());
            assert!(worst >= RAD - 0.5, "{map}: a marble passes through scenery ({worst})");
            eprintln!("{map}: off-grid hits {off_grid}, beats with a hit {covered}/{}, all six together on {:.0}% of beats, pad kinds {} (orbs {:.0}%), finish {:.2}s, closest pair {:.0}px, top speed {:.0}", bs.len(), unison * 100.0, kinds.len(), orbs * 100.0, tl.win_time, min_gap, max_v);
            assert_eq!(off_grid, 0, "{map}: hits must sit on the beat grid");
            assert!(covered as f64 >= bs.len() as f64 * 0.95, "{map}: beats without any hit");
            assert!(unison < 0.25, "{map}: marbles too much in unison ({unison})");
            assert!(kinds.len() >= 5 && orbs <= 0.28, "{map}: not enough variety ({} kinds, {orbs} orbs)", kinds.len());
            assert!((tl.win_time - 27.5).abs() < 1.2, "{map}: finish {}", tl.win_time);
            assert_eq!(tl.sim.winner, 3, "{map}: winner");
            assert!(min_gap >= 2.0 * RAD - 1.0, "{map}: marbles overlap ({min_gap})");
            assert!(max_v <= 1400.0, "{map}: speed {max_v}");
        }
    }

    #[test]
    fn designed_map_has_everyone_on_the_beat() {
        let beats: Vec<f64> = (0..70).map(|i| 0.4 + i as f64 * 60.0 / 128.0).collect();
        let bars: Vec<(f64, f32)> = (0..18).map(|i| (0.4 + i as f64 * 4.0 * 60.0 / 128.0, [0.1f32, 0.2, 0.4, 0.6, 0.9, 1.0, 1.0, 0.9, 0.5, 0.3, 0.2, 0.2, 0.7, 0.9, 1.0, 0.8, 0.4, 0.2][i])).collect();
        let mut grid: Vec<f64> = vec![];
        for w in beats.windows(2) { for q in 0..4 { grid.push(w[0] + (w[1] - w[0]) * q as f64 / 4.0); } } // sixteenth notes
        let plan = [(8.0, 2usize), (16.0, 4usize)];
        let seeds = 12;
        let (mut strong, mut on2) = (0usize, 0usize);
        let (mut on_time, mut changes_ok, mut clear) = (0, 0, 0);
        for k in 0..seeds {
            let seed = format!("seed{k}");
            let cfg = crate::designed::DesignCfg { beats: &beats, bars: &bars, n: 6, seed: &seed, map: "neon", len: 30.0, plan: &plan, winner: Some(3) };
            let d = crate::designed::build(&cfg).expect("built");
            let near = |t: f64| grid.iter().map(|g| (g - t).abs()).fold(f64::MAX, f64::min);
            // every marble's real impacts are on the sixteenth-note grid
            let ev: Vec<&Event> = d.sim.events.iter().filter(|e| e.v >= 250.0 && e.kind != 3 && e.f as f64 / FPS > 2.0 && (e.f as f64 / FPS) < d.sim.twin - 0.5).collect();
            strong += ev.len(); on2 += ev.iter().filter(|e| near(e.f as f64 / FPS) <= 2.0 / FPS + 1e-6).count();
            // never more than three marbles hit at the same moment
            let mut crowd = 0usize;
            for e in &ev { let c = ev.iter().filter(|o| (o.f as i64 - e.f as i64).abs() <= 2).map(|o| o.m).collect::<std::collections::BTreeSet<_>>().len(); crowd = crowd.max(c); }
            // the lead changes hands, the winner is exact, everyone finishes, the finish is on time
            let finished = d.sim.fins.iter().filter(|f| **f >= 0.0).count();
            let changes = (1..d.sim.frames).filter(|&f| d.sim.lead[f] != d.sim.lead[f - 1]).count();
            // marbles must not sit on top of each other, and must move at believable speeds
            let (mut max_v, mut touching, mut pairs) = (0.0f64, 0usize, 0usize);
            let race_end = ((d.sim.twin + 0.3) * FPS) as usize;
            for f in 1..d.sim.frames.min(race_end) { for a in 0..6 { let (ax, ay) = d.sim.at(f, a); let (px, py) = d.sim.at(f - 1, a); max_v = max_v.max((ax - px).hypot(ay - py) * FPS); for b in (a + 1)..6 { let (bx, by) = d.sim.at(f, b); pairs += 1; if (ax - bx).hypot(ay - by) < 20.0 { touching += 1; } } } }
            let tp = 100.0 * touching as f64 / pairs.max(1) as f64;
            eprintln!("{seed}: impacts {} (on grid ±2 frames {}), most marbles hitting at once {crowd}, finished {finished}/6, winner {}, lead changes {changes}, finish {:.2}s, touching {tp:.1}%, top speed {max_v:.0}", ev.len(), ev.iter().filter(|e| near(e.f as f64 / FPS) <= 2.0 / FPS + 1e-6).count(), d.sim.winner, d.sim.twin);
            assert_eq!(d.sim.winner, 3, "{seed}: the planned winner must win");
            assert!(crowd <= 3, "{seed}: {crowd} marbles hit at once");
            assert!(max_v <= 1500.0, "{seed}: speed {max_v}");
            assert!(tp < 6.0, "{seed}: marbles on top of each other {tp}% of the time");
            if finished == 6 && (d.sim.twin - 27.5).abs() < 1.2 { on_time += 1; }
            if changes >= 2 { changes_ok += 1; }
            if tp < 1.0 { clear += 1; }
        }
        eprintln!("overall: impacts on the grid {on2}/{strong}; on time {on_time}/{seeds}; lead changes hands {changes_ok}/{seeds}; clear of each other {clear}/{seeds}");
        assert!(on2 as f64 >= strong as f64 * 0.9, "only {on2}/{strong} impacts on the grid");
        assert!(on_time as f64 >= seeds as f64 * 0.8, "only {on_time}/{seeds} finished on time with everyone across");
        assert!(changes_ok as f64 >= seeds as f64 * 0.8, "the lead changed hands in only {changes_ok}/{seeds}");
        assert!(clear as f64 >= seeds as f64 * 0.7, "marbles were clear of each other in only {clear}/{seeds}");
    }

    #[test]
    fn warp_puts_impacts_on_the_beat() {
        let beats: Vec<f64> = (0..70).map(|i| 0.4 + i as f64 * 60.0 / 128.0).collect();
        for map in ["neon", "pachinko", "slide", "spinner", "funnel", "chaos"] {
            let tl = build_timeline(&RaceSetup { seed: "sync-test", map, n: 6, len: 30.0, beats: &beats, plan: &[], winner: None, bars: &[], compose: false, crowd: 0, designed: false });
            let (n, before, after, lo, hi) = alignment_stats(&tl, &beats);
            let (cb, ca) = beat_coverage(&tl, &beats);
            eprintln!("{map}: beats with a hit on them {:.0}% -> {:.0}%", cb * 100.0, ca * 100.0);
            eprintln!("{map}: {n} strong impacts, on-beat before {:.0}% after {:.0}%, speed {:.2}x-{:.2}x", 100.0 * before as f64 / n as f64, 100.0 * after as f64 / n as f64, lo, hi);
            assert!(after > before, "{map}: warping must help");
            assert!(lo > 0.6 && hi < 1.6, "{map}: playback speed varies too much {lo} {hi}");
        }
    }
}
