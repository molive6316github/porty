//! Marble physics: a port of the web version's simulation (same constants, same order of operations).
use crate::course::*;
use crate::rng::{hash_seed, Rng};

pub const FPS: f64 = 60.0;
pub const GRAV: f64 = 1000.0;
const DT: f64 = 1.0 / 120.0;

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
}

#[derive(Clone, Copy, Debug)]
pub struct Event {
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
}

impl Sim {
    pub fn at(&self, f: usize, i: usize) -> (f64, f64) {
        let k = (f * self.n + i) * 2;
        (self.pos[k] as f64, self.pos[k + 1] as f64)
    }
}

fn hit_seg(m: &mut Marble, x1: f64, y1: f64, x2: f64, y2: f64, r: f64, e: f64) -> f64 {
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
        return -vn;
    }
    0.0
}

pub fn simulate(course: &Course, n: usize, beats: &[f64], nudge_seed: u32, seed: &str, max_t: f64, sched: Option<&[SchedKey]>) -> Sim {
    let mut r0 = Rng::new(hash_seed(&format!("{seed}|start")));
    let mut rn = Rng::new(nudge_seed);
    let mut ms: Vec<Marble> = Vec::with_capacity(n);
    for i in 0..n {
        let (row, col, per) = (i % 2, i / 2, (n + 1) / 2);
        let x = 70.0 + (W - 140.0) * ((col as f64 + if row == 1 { 0.5 } else { 0.0 } + r0.next() * 0.2) / (per as f64 - 0.5).max(1.0));
        let vx = (r0.next() - 0.5) * 30.0;
        ms.push(Marble { x, y: 50.0 + row as f64 * 36.0, vx, vy: 0.0, fin: -1.0, still: 0, last_ev: -9, last_mm: -9 });
    }
    let max_f = (max_t * FPS).ceil() as usize + 1;
    let mut pos = vec![0f32; max_f * n * 2];
    let mut lead = vec![0u8; max_f];
    let mut cam = vec![0f32; max_f];
    let (mut f, mut bi) = (0usize, 0usize);
    let mut cam_y = 300.0f64;
    let (mut winner, mut done, mut end_f, mut win_f): (i64, usize, i64, i64) = (-1, 0, -1, -1);
    let mut di: i64 = -1;
    let mut events: Vec<Event> = Vec::new();
    let finish_y = course.finish_y;
    while f < max_f {
        let t = f as f64 / FPS;
        for i in 0..n {
            pos[(f * n + i) * 2] = ms[i].x as f32;
            pos[(f * n + i) * 2 + 1] = ms[i].y as f32;
        }
        let (mut l, mut best) = (0usize, f64::NEG_INFINITY);
        for i in 0..n {
            let sc = if ms[i].fin >= 0.0 { 1e9 - ms[i].fin } else { ms[i].y };
            if sc > best {
                best = sc;
                l = i;
            }
        }
        lead[f] = l as u8;
        let ly = ms.iter().map(|m| m.y).fold(f64::NEG_INFINITY, f64::max);
        cam_y += (ly.min(finish_y - 80.0) - cam_y) * 0.1;
        cam[f] = cam_y as f32;
        if win_f >= 0 && end_f < 0 && f as i64 >= win_f + (3.0 * FPS).floor() as i64 {
            end_f = f as i64;
        }
        if done == n && end_f < 0 {
            end_f = f as i64 + FPS as i64;
        }
        if end_f >= 0 && f as i64 >= end_f {
            f += 1;
            break;
        }
        while bi < beats.len() && beats[bi] <= t {
            let k = (rn.next() * n as f64).floor() as usize;
            ms[k].vx += (rn.next() - 0.5) * 300.0;
            ms[k].vy -= 40.0 * rn.next();
            bi += 1;
        }
        for s in 0..2 {
            let st = t + s as f64 * DT;
            if let Some(sc) = sched {
                // the director: push the marble that should lead forward, hold the current leader back
                while ((di + 1) as usize) < sc.len() && sc[(di + 1) as usize].ts <= st {
                    di += 1;
                }
                if di >= 0 {
                    let d = sc[di as usize].m;
                    let (mut lm, mut ly2) = (-1i64, f64::NEG_INFINITY);
                    for q in 0..n {
                        if ms[q].fin < 0.0 && ms[q].y > ly2 {
                            ly2 = ms[q].y;
                            lm = q as i64;
                        }
                    }
                    if lm >= 0 && ms[d].fin < 0.0 {
                        if lm as usize != d {
                            let gap = ly2 - ms[d].y;
                            let k = (0.6 + gap / 70.0).clamp(0.6, 4.0);
                            ms[d].vy += GRAV * DT * 1.1 * k;
                            let lmi = lm as usize;
                            ms[lmi].vy -= GRAV * DT * 0.9 * k.min(2.5);
                            ms[lmi].vy *= 1.0 - 0.006 * k.min(3.0);
                        } else {
                            ms[d].vy += GRAV * DT * 0.12;
                            let dy = ms[d].y;
                            for q in 0..n {
                                if q != d && ms[q].fin < 0.0 && dy - ms[q].y > 220.0 {
                                    ms[q].vy += GRAV * DT * 0.25;
                                }
                            }
                        }
                    }
                }
            }
            for mi in 0..n {
                let mut m = ms[mi];
                m.vy += GRAV * DT;
                let sp = m.vx.hypot(m.vy);
                if sp > 1300.0 {
                    m.vx *= 1300.0 / sp;
                    m.vy *= 1300.0 / sp;
                }
                m.x += m.vx * DT;
                m.y += m.vy * DT;
                if m.x < RAD {
                    m.x = RAD;
                    m.vx = m.vx.abs() * 0.5;
                } else if m.x > W - RAD {
                    m.x = W - RAD;
                    m.vx = -m.vx.abs() * 0.5;
                }
                let bidx = ((m.y / BAND).floor() as i64).clamp(0, course.bands.len() as i64 - 1) as usize;
                for &si in &course.bands[bidx] {
                    let sg = &course.segs[si as usize];
                    let imp = hit_seg(&mut m, sg.x1, sg.y1, sg.x2, sg.y2, sg.r, sg.e);
                    if imp > 140.0 && f as i64 - m.last_ev > 2 {
                        m.last_ev = f as i64;
                        let kind = match sg.kind {
                            Kind::Peg => 0,
                            Kind::Bump => 1,
                            Kind::Ramp => 2,
                        };
                        events.push(Event { f: f as u32, kind, v: imp as f32, x: m.x as f32 });
                    }
                }
                for sp2 in &course.spins {
                    if (m.y - sp2.cy).abs() < sp2.len + 30.0 {
                        let a = sp2.ph + sp2.w * st;
                        let (c, sn) = (a.cos() * sp2.len, a.sin() * sp2.len);
                        let (px, py) = (m.x, m.y);
                        hit_seg(&mut m, sp2.cx - c, sp2.cy - sn, sp2.cx + c, sp2.cy + sn, sp2.r, 0.5);
                        if m.x != px || m.y != py {
                            let (rx, ry) = (m.x - sp2.cx, m.y - sp2.cy);
                            m.vx += -ry * sp2.w * 0.35 * DT * 60.0 * 0.1;
                            m.vy += rx * sp2.w * 0.35 * DT * 60.0 * 0.1;
                        }
                    }
                }
                if m.vx.hypot(m.vy) < 12.0 && m.y < finish_y {
                    m.still += 1;
                    if m.still > 150 {
                        m.vx += (if m.x < W / 2.0 { 1.0 } else { -1.0 }) * (220.0 + rn.next() * 160.0);
                        m.vy -= 160.0;
                        m.still = 0;
                    }
                } else {
                    m.still = 0;
                }
                if m.fin < 0.0 && m.y > finish_y {
                    m.fin = st;
                    done += 1;
                    if winner < 0 {
                        winner = mi as i64;
                        win_f = f as i64;
                    }
                }
                ms[mi] = m;
            }
            for i in 0..n {
                for j in (i + 1)..n {
                    let (a, b) = (ms[i], ms[j]);
                    let (mut dx, mut dy) = (b.x - a.x, b.y - a.y);
                    let d2 = dx * dx + dy * dy;
                    let rr = RAD * 2.0;
                    if d2 < rr * rr && d2 > 1e-9 {
                        let d = d2.sqrt();
                        dx /= d;
                        dy /= d;
                        let ov = (rr - d) / 2.0;
                        let (mut a, mut b) = (a, b);
                        a.x -= dx * ov;
                        a.y -= dy * ov;
                        b.x += dx * ov;
                        b.y += dy * ov;
                        let vn = (b.vx - a.vx) * dx + (b.vy - a.vy) * dy;
                        if vn < 0.0 {
                            let j2 = -(1.0 + 0.6) * vn / 2.0;
                            a.vx -= j2 * dx;
                            a.vy -= j2 * dy;
                            b.vx += j2 * dx;
                            b.vy += j2 * dy;
                            if -vn > 160.0 && f as i64 - a.last_mm > 3 {
                                a.last_mm = f as i64;
                                events.push(Event { f: f as u32, kind: 3, v: (-vn) as f32, x: ((a.x + b.x) / 2.0) as f32 });
                            }
                        }
                        ms[i] = a;
                        ms[j] = b;
                    }
                }
            }
        }
        f += 1;
    }
    if winner < 0 {
        let mut b = 0;
        for i in 1..n {
            if ms[i].y > ms[b].y {
                b = i;
            }
        }
        winner = b as i64;
    }
    let w = winner as usize;
    let twin = if ms[w].fin >= 0.0 { ms[w].fin } else { max_t };
    pos.truncate(f * n * 2);
    lead.truncate(f);
    cam.truncate(f);
    Sim { n, frames: f, pos, lead, cam, twin, winner: w, fins: ms.iter().map(|m| m.fin).collect(), events }
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
}

impl Timeline {
    pub fn sim_frame(&self, f: f64) -> f64 {
        (f * self.factor).min(self.sim.frames as f64 - 1.0).max(0.0)
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
}

pub fn build_timeline(s: &RaceSetup) -> Timeline {
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
        for k in 0..16u32 {
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
    let frames = (len * FPS).ceil() as usize;
    let lead_a: Vec<u8> = (0..=frames).map(|f| sim.lead[((f as f64 * factor).floor() as usize).min(sim.frames - 1)]).collect();
    let win_time = sim.twin / factor;
    Timeline { course, sim, factor, frames, lead_a, win_time, n, len, landed }
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
