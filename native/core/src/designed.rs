//! Designed map mode: a real course of neon ramps and trampolines composed from the music, with every marble choreographed on it.
//!
//! One reference marble is simulated alone. The map is built along its path so it rolls, falls, lands and bounces exactly on the
//! beat: it rolls down a ramp for N beats, the ramp ends under it, and the next ramp (or trampoline) is placed exactly where it will
//! be one or two beats later. Trampolines launch at a speed that makes each bounce last exactly one beat.
//!
//! Every marble then rides that same path, staggered by half-beats, so every landing and bounce of every marble is on the beat grid,
//! each marble on its own subdivision. Choreography on top: marbles fan out sideways while falling and reconverge at the landing,
//! overtakes are leapfrogs (the passing marble hops over), the start is a row of marbles released one after another, and bounce
//! chains become canons. Who leads is decided directly, so the winner and planned leaders are exact.
use crate::course::*;
use crate::rng::{hash_seed, Rng};
use crate::sim::*;

pub struct DesignCfg<'a> {
    pub beats: &'a [f64],
    pub bars: &'a [(f64, f32)],
    pub n: usize,
    pub seed: &'a str,
    pub map: &'a str,
    pub len: f64,
    pub plan: &'a [(f64, usize)],
    pub winner: Option<usize>,
}

pub struct Designed { pub course: Course, pub sim: Sim, pub landings: Vec<f64>, pub kinds: Vec<u8> }

fn inten(bars: &[(f64, f32)], t: f64) -> f64 {
    if bars.is_empty() { return 0.6; }
    let j = bars.partition_point(|b| b.0 <= t);
    bars[j.saturating_sub(1)].1 as f64
}

/// the marble's state after `frames` frames of free flight, using exactly the engine's integration and wall rules
fn predict(mut x: f64, mut y: f64, mut vx: f64, mut vy: f64, frames: usize) -> (f64, f64, f64, f64) {
    for _ in 0..frames {
        for _ in 0..2 {
            vy += GRAV * DT;
            let sp = vx.hypot(vy);
            if sp > 1300.0 { vx *= 1300.0 / sp; vy *= 1300.0 / sp; }
            x += vx * DT; y += vy * DT;
            if x < RAD { x = RAD; vx = vx.abs() * 0.5; } else if x > W - RAD { x = W - RAD; vx = -vx.abs() * 0.5; }
        }
    }
    (x, y, vx, vy)
}

#[derive(Clone, Copy)]
enum Module { Roll { beats: f64, fall: f64 }, Bounce { m: usize } }
#[derive(Clone, Copy)]
enum Phase { Roll { seg: usize, cut_t: f64, fall: f64 }, Bounce { last_t: f64 } }

fn beat_at_or_after(beats: &[f64], t: f64) -> f64 {
    let j = beats.partition_point(|b| *b < t - 1e-9);
    beats.get(j).copied().unwrap_or(t)
}

fn choose(rng: &mut Rng, i: f64, allow_bounce: bool) -> Module {
    let r = rng.next();
    if allow_bounce && i >= 0.62 && r < 0.5 { return Module::Bounce { m: 2 + (rng.next() * 3.0) as usize }; }
    let beats = if i < 0.35 { 2.0 + (rng.next() * 2.0).floor() } else if i < 0.7 { 1.0 + (rng.next() * 2.0).floor() } else { 1.0 };
    let fall = if i < 0.45 { 1.0 } else if rng.next() < 0.6 { 2.0 } else { 1.0 };
    Module::Roll { beats, fall }
}

fn smooth(u: f64) -> f64 { let u = u.clamp(0.0, 1.0); u * u * (3.0 - 2.0 * u) }

/// a marble's delay behind the reference marble: constant, replaced in one step when it swaps places with another marble
#[derive(Clone, Default)]
struct Delay { start: f64, steps: Vec<(f64, f64)> } // (time the new value takes over, the value)
impl Delay {
    fn at(&self, t: f64) -> f64 { let mut v = self.start; for s in &self.steps { if t >= s.0 { v = s.1; } else { break; } } v }
}

/// two marbles exchange places by leaping over each other: `a` (the overtaker) arcs up and over while `b` slides underneath
#[derive(Clone, Copy)]
struct Leap { t0: f64, t1: f64, a: usize, b: usize, d_a: f64, d_b: f64 }

/// The marbles are physically identical, so the planned winner and leaders are settled by choosing which marble's recording carries
/// which name: the winner's name goes to whoever wins, and each planned leader's name to whoever leads then.
fn relabel(sim: &mut Sim, cfg: &DesignCfg) {
    let n = cfg.n;
    let mut label_of: Vec<Option<usize>> = vec![None; n];
    let mut used = vec![false; n];
    if let Some(w) = cfg.winner.filter(|w| *w < n) { label_of[sim.winner] = Some(w); used[w] = true; }
    for &(t, m) in cfg.plan {
        if m >= n || used[m] { continue; }
        let j = sim.lead[(((t + 3.0) * FPS) as usize).min(sim.frames - 1)] as usize;
        if label_of[j].is_none() { label_of[j] = Some(m); used[m] = true; }
    }
    let mut free: Vec<usize> = (0..n).filter(|m| !used[*m]).collect();
    let perm: Vec<usize> = (0..n).map(|j| match label_of[j] { Some(l) => l, None => free.remove(0) }).collect();
    let (nf, op, ot, orank) = (sim.frames, sim.pos.clone(), sim.touch.clone(), sim.rank.clone());
    for f in 0..nf { for j in 0..n { let l = perm[j];
        sim.pos[(f * n + l) * 2] = op[(f * n + j) * 2]; sim.pos[(f * n + l) * 2 + 1] = op[(f * n + j) * 2 + 1];
        sim.touch[f * n + l] = ot[f * n + j]; sim.rank[f * n + l] = orank[f * n + j]; } }
    for f in 0..nf { sim.lead[f] = perm[sim.lead[f] as usize] as u8; }
    for e in sim.events.iter_mut() { if (e.m as usize) < n { e.m = perm[e.m as usize] as u8; } }
    let of = sim.fins.clone();
    for j in 0..n { sim.fins[perm[j]] = of[j]; }
    sim.winner = perm[sim.winner];
}

pub fn build(cfg: &DesignCfg) -> Option<Designed> {
    let n = cfg.n;
    let target = cfg.len - 2.5;
    if cfg.beats.len() < 8 || n < 1 { return None; }
    let period = { let mut v: Vec<f64> = cfg.beats.windows(2).map(|w| w[1] - w[0]).collect(); v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[v.len() / 2].max(0.25) };
    let nudge = hash_seed(&format!("{}|n", cfg.seed));
    let mut rng = Rng::new(hash_seed(&format!("{}|designed|{}", cfg.seed, cfg.map)));
    let kick = 0.5 * GRAV * period * 1.05; // a bounce launched at this speed lasts exactly one beat (calibrated for the engine's step)
    let mut course = Course::blank(cfg.map, cfg.seed, 900);
    let r0 = course.add_seg(8.0, 250.0, W - 8.0, 250.0 + 0.16 * (W - 16.0), 5.0, 0.12, 0, 0.0, 0.0);
    let mut prev_dir = 1.0f64;
    let mut phase = Phase::Roll { seg: r0, cut_t: beat_at_or_after(cfg.beats, 2.4), fall: 1.0 };
    let mut landings: Vec<f64> = vec![];
    let mut kinds: Vec<u8> = vec![]; // 0 = ramp landing, 1 = trampoline first landing, 2 = trampoline bounce
    // ----- pass 1: one reference marble, composing the map along its path
    let mut e = Engine::new(1, nudge, cfg.seed);
    let max_t = target + 3.8;
    let max_f = (max_t * FPS) as usize + 2;
    let mut finished_planning = false;
    while e.f < max_f {
        let t = e.time();
        let wake = match phase {
            Phase::Roll { seg, cut_t, .. } => {
                let (x, y, hvx, hvy, _) = e.state(0);
                let g = &course.segs[seg];
                let (dx, dy) = (g.x2 - g.x1, g.y2 - g.y1);
                let l = dx.hypot(dy).max(1e-9);
                let remaining = l - ((x - g.x1) * dx + (y - g.y1) * dy) / l;
                let downhill = (hvx * dx + hvy * dy) / l > 20.0;
                // at the beat while rolling downhill, or sooner if about to run out of ramp (the landing is still on a beat)
                (t >= cut_t - 1e-9 && (downhill || t >= cut_t + 0.8)) || (remaining < 34.0 && e.touching(0))
            }
            Phase::Bounce { last_t } => t >= last_t + 1.0 / FPS,
        };
        if wake && !finished_planning {
            let (x, y, vx, vy, _) = e.state(0);
            let (fall, land_floor);
            match phase {
                Phase::Roll { seg, fall: f, .. } => {
                    let g = &mut course.segs[seg];
                    let (dx, dy) = (g.x2 - g.x1, g.y2 - g.y1);
                    let l = dx.hypot(dy).max(1e-9);
                    // cut a marble-and-a-bit behind so it is clear of the end and drops at once, and always leave a gap to the nearer
                    // wall wide enough to fall through
                    let u = ((x - g.x1) * dx + (y - g.y1) * dy) / l - (RAD + 8.0);
                    let mut u = u.clamp(10.0, l);
                    let ex = g.x1 + dx / l * u;
                    if ex > W - 42.0 || ex < 42.0 {
                        let target_x = ex.clamp(42.0, W - 42.0);
                        if dx.abs() > 1e-6 { u = ((target_x - g.x1) / (dx / l)).clamp(10.0, l); }
                    }
                    g.x2 = g.x1 + dx / l * u; g.y2 = g.y1 + dy / l * u;
                    fall = f; land_floor = t + 0.3;
                }
                Phase::Bounce { last_t } => { fall = 2.0; land_floor = last_t + 1.5 * period; }
            }
            let mut land_t = beat_at_or_after(cfg.beats, (t + fall * period * 0.9).max(land_floor));
            let last = land_t >= target - 0.02;
            if last { land_t = target; }
            else {
                // prefer a landing beat whose spot leaves plenty of room in the direction of travel: a marble that lands next to a wall
                // moving toward it climbs the slope, stalls and wobbles there
                let room_at = |lt: f64| -> f64 { let fr = ((lt - t) * FPS).round().max(1.0) as usize; let (px, _, pvx, _) = predict(x, y, vx, vy, fr); if pvx > 60.0 { W - 6.0 - px } else if pvx < -60.0 { px - 6.0 } else { px.min(W - px) + 80.0 } };
                let mut best = (room_at(land_t), land_t);
                if best.0 < 190.0 { for k in 1..=2 { let c = beat_at_or_after(cfg.beats, land_t + 0.01 + (k as f64 - 1.0) * period) + if k == 1 { period * 0.0 } else { 0.0 }; let c = c.max(land_t + period * k as f64 * 0.99); if c < target - 0.6 { let r = room_at(c); if r > best.0 { best = (r, c); } } } }
                land_t = beat_at_or_after(cfg.beats, best.1 - 0.01);
                if land_t >= target - 0.02 { land_t = target; }
            }
            let last = land_t >= target - 0.02;
            let frames = ((land_t - t) * FPS).round().max(1.0) as usize;
            let (px, py, pvx_land, pvy_land) = predict(x, y, vx, vy, frames);
            landings.push(land_t);
            kinds.push(0);
            // no trampoline chain in the last stretch: the race must end on a real fall
            let allow_bounce = land_t < target - 8.0 * period;
            let near_plan = cfg.plan.iter().any(|p| p.0 > land_t - 1.0 && p.0 < land_t + 3.5) || (cfg.winner.is_some() && land_t > target - 34.0 * period && rng.next() < 0.6);
            let module = if last { Module::Roll { beats: 99.0, fall: 1.0 } } else if near_plan { Module::Roll { beats: 2.0, fall: 2.0 } } else { choose(&mut rng, inten(cfg.bars, land_t), allow_bounce) };
            match module {
                Module::Roll { beats, fall } => {
                    let alpha = 0.2 + 0.14 * rng.next();
                    let pref = if pvx_land.abs() > 80.0 { pvx_land.signum() } else { -prev_dir };
                    // pick the direction and roll length that leave room to fall without reaching a wall
                    let mut chosen: Option<(f64, f64, f64)> = None; // dir, beats, alpha
                    'search: for &dir in &[pref, -pref] {
                        for &al in &[alpha, alpha * 0.7] {
                            let (c, sn) = (al.cos(), al.sin());
                            let vt = pvx_land * dir * c + pvy_land * sn;
                            if last && vt < 60.0 { continue; } // the race ends with a roll downhill, so the finish is crossed at once
                            let acc = GRAV * sn * 0.97;
                            let room = if dir > 0.0 { W - 6.0 - px } else { px - 6.0 };
                            let mut nb = beats;
                            loop {
                                let tt = nb * period;
                                let dist = vt.max(0.0) * tt + 0.5 * acc * tt * tt;
                                let vend = vt.max(0.0) + acc * tt;
                                if dist * c + vend * c * fall * period + 55.0 <= room { chosen = Some((dir, nb, al)); break 'search; }
                                if nb <= 1.0 { break; }
                                nb -= 1.0;
                            }
                        }
                    }
                    let (dirx, nb, alpha) = chosen.unwrap_or(((if px < W / 2.0 { 1.0 } else { -1.0 }), 1.0, alpha * 0.6));
                    let (c, s) = (alpha.cos(), alpha.sin());
                    let tan = (dirx * c, s);
                    let n1 = (tan.1, -tan.0);
                    let nu = if n1.1 < 0.0 { n1 } else { (-n1.0, -n1.1) };
                    let sp = (px - nu.0 * (RAD + 5.0), py - nu.1 * (RAD + 5.0));
                    let lu = if dirx > 0.0 { (sp.0 - 6.0) / c } else { (W - 6.0 - sp.0) / c }.max(40.0);
                    let ld = if dirx > 0.0 { (W - 6.0 - sp.0) / c } else { (sp.0 - 6.0) / c }.max(60.0);
                    let seg = course.add_seg(sp.0 - tan.0 * lu, sp.1 - tan.1 * lu, sp.0 + tan.0 * ld, sp.1 + tan.1 * ld, 5.0, 0.12, 0, 0.0, 0.0);
                    prev_dir = dirx;
                    phase = Phase::Roll { seg, cut_t: land_t + nb * period, fall };
                }
                Module::Bounce { m } => {
                    let ds = if px < W / 2.0 { 1.0 } else { -1.0 };
                    let bv = 105.0 + 55.0 * rng.next();
                    let mut m = m;
                    while m > 2 && (px + ds * (bv * (m - 1) as f64 * period + 10.0) > W - 30.0 || px + ds * (bv * (m - 1) as f64 * period + 10.0) < 30.0) { m -= 1; }
                    let reach = bv * (m - 1) as f64 * period + 10.0;
                    let sy = py + RAD + 5.0;
                    course.add_seg(px - ds * 70.0, sy, px + ds * reach, sy, 5.0, 0.1, shape::TRAMP, kick, ds * bv);
                    *kinds.last_mut().unwrap() = 1;
                    for k in 1..m { landings.push(land_t + k as f64 * period); kinds.push(2); }
                    phase = Phase::Bounce { last_t: land_t + (m - 1) as f64 * period };
                }
            }
            if last { finished_planning = true; }
        }
        if !e.step(&course, None) { break; }
    }
    let reference = e.finish(max_t);
    let rf = reference.frames;
    if std::env::var("MARBEAT_DEBUG_DESIGN").is_ok() {
        let mut f = 1; while f < rf { let sp = |k: usize| { let (a, b) = (reference.at(k - 1, 0), reference.at(k, 0)); (a.0 - b.0).hypot(a.1 - b.1) * FPS }; if sp(f) < 30.0 { let s0 = f; while f < rf && sp(f) < 30.0 { f += 1; } if f - s0 >= 8 { eprintln!("reference stalls {:.2}s-{:.2}s at ({:.0},{:.0}) touch {}", s0 as f64 / FPS, f as f64 / FPS, reference.at(s0, 0).0, reference.at(s0, 0).1, reference.touch[s0]); } } else { f += 1; } }
    }
    // the finish line sits just below the reference marble's last landing, and a floor catches everyone after it
    let finish_y = reference.at(((target * FPS).round() as usize).min(rf - 1), 0).1 + 3.0;
    course.finish_y = finish_y;
    course.floor_y = finish_y + 170.0;
    course.add_seg(0.0, finish_y + 170.0, W, finish_y + 170.0, 6.0, 0.2, 0, 0.0, 0.0);
    let pref = |ts: f64| -> (f64, f64) {
        let x = (ts * FPS).max(0.0);
        let i = (x as usize).min(rf - 1);
        let j = (i + 1).min(rf - 1);
        let u = (x - i as f64).clamp(0.0, 1.0);
        let (a, b) = (reference.at(i, 0), reference.at(j, 0));
        (a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u)
    };
    let t_ref_end = (rf - 1) as f64 / FPS;

    // ----- who is where in the line: each marble follows the reference path at its own delay, changing only in smooth windows
    // Two copies of the path at different delays are close wherever the path is slow or revisits a spot. For any delay gap, measure the
    // closest two copies ever come (while the race is on), and only accept gaps that keep a safe distance.
    let self_dist = |gap: f64| -> f64 {
        let g = (gap * FPS).round() as usize;
        let (lo, hi) = ((0.6 * FPS) as usize, ((target + 0.05) * FPS) as usize);
        let mut m = f64::MAX;
        for f in lo..hi.min(rf.saturating_sub(g + 1)) { let (a, b) = (reference.at(f, 0), reference.at(f + g, 0)); m = m.min((a.0 - b.0).hypot(a.1 - b.1)); }
        m
    };
    if std::env::var("MARBEAT_DEBUG_DESIGN").is_ok() {
        for k in 1..=12 { let gap = k as f64 * 0.25 * period; let g = (gap * FPS).round() as usize; let (lo, hi) = ((1.2 * FPS) as usize, ((target + 1.0) * FPS) as usize);
            let mut m = (f64::MAX, 0usize); for f in lo..hi.min(rf.saturating_sub(g + 1)) { let (a, b) = (reference.at(f, 0), reference.at(f + g, 0)); let d = (a.0 - b.0).hypot(a.1 - b.1); if d < m.0 { m = (d, f); } }
            eprintln!("gap {:.2}s ({:.2} beats): closest {:.0}px at ref t={:.2}s", gap, gap / period, m.0, m.1 as f64 / FPS); }
    }
    // slot delays on the sixteenth-note grid, at least half a beat apart, chosen one after another so that every gap between two slots
    // keeps the marbles clear of each other. A swap only exchanges two marbles' slots, so these gaps never change.
    let span_limit = 1.3; // seconds the last marble may trail by
    let slots: Vec<f64> = {
        let mut best: Vec<f64> = vec![];
        'margins: for &need in &[34.0f64, 30.0, 27.0, 22.0, 14.0, 0.0] {
            let mut sl: Vec<f64> = vec![0.0];
            for _ in 1..n {
                let mut d = sl.last().unwrap() + 0.25 * period;
                let mut ok = false;
                while d <= span_limit {
                    if sl.iter().all(|&e| self_dist(d - e) >= need) { ok = true; break; }
                    d += 0.25 * period;
                }
                if !ok { continue 'margins; }
                sl.push(d);
            }
            best = sl;
            break;
        }
        if best.is_empty() { best = (0..n).map(|i| i as f64 * 0.75 * period).collect(); }
        best
    };
    // start the line in the order the lead has to change, so every planned change is a single swap of neighbours:
    // someone else in front, then each planned leader in turn, then the winner, then the rest
    let mut seq: Vec<usize> = vec![];
    for p in cfg.plan.iter().filter(|p| p.1 < n) { if !seq.contains(&p.1) { seq.push(p.1); } }
    if let Some(w) = cfg.winner.filter(|w| *w < n) { if !seq.contains(&w) { seq.push(w); } }
    let early = cfg.plan.iter().any(|p| p.1 < n && p.0 <= 1.0); // a plan that starts at once leads from the start
    let mut shuffled: Vec<usize> = (0..n).collect();
    for k in (1..n).rev() { let j = (rng.next() * (k + 1) as f64) as usize; shuffled.swap(k, j.min(k)); }
    let mut order: Vec<usize> = vec![];
    if !early { if let Some(x) = shuffled.iter().copied().find(|m| !seq.contains(m)) { order.push(x); } }
    for m in &seq { order.push(*m); }
    for m in &shuffled { if !order.contains(m) { order.push(*m); } }
    let mut q: Vec<i32> = vec![0; n];
    for (pos, m) in order.iter().enumerate() { q[*m] = pos as i32; }
    let plan_at = |t: f64| -> Option<usize> {
        if let Some(w) = cfg.winner.filter(|w| *w < n) { if t > target - 34.0 * period { return Some(w); } }
        cfg.plan.iter().filter(|p| p.0 <= t + 1.0 && p.1 < n).last().map(|p| p.1)
    };
    // ----- flights of the reference marble (airborne between two contacts): marbles fan out sideways and come back together
    let mut flights: Vec<(f64, f64)> = vec![];
    { let mut f = 0;
      while f < rf { if reference.touch[f] == 0 { let s = f; while f < rf && reference.touch[f] == 0 { f += 1; } if f - s >= 10 && s > 0 && f < rf { flights.push((s as f64 / FPS, f as f64 / FPS)); } } else { f += 1; } } }
    let t_cross_ref = (((target - 0.3) * FPS) as usize..rf).find(|&f| reference.at(f, 0).1 > finish_y).map(|f| f as f64 / FPS).unwrap_or(target);
    if std::env::var("MARBEAT_DEBUG_DESIGN").is_ok() { eprintln!("ref frames {} ({:.2}s) target {:.2} finish_y {:.0} crosses at {:.2}s; y(target) {:.0}; slots {:?}", rf, rf as f64 / FPS, target, finish_y, t_cross_ref, reference.at(((target * FPS) as usize).min(rf - 1), 0).1, slots); }
    let first_contact = flights.first().map(|f| f.0).unwrap_or(0.8).max(0.3);
    let gate_x = |i: usize| 70.0 + (W - 140.0) * (i as f64 + 0.5) / n as f64;
    let start_pos = pref(0.0);
    // per marble, per flight: how far it fans out (kept small near the walls)
    let fan: Vec<Vec<f64>> = (0..n).map(|i| (0..flights.len()).map(|k| { let hsh = hash_seed(&format!("{}|fan|{i}|{k}", cfg.seed)); ((hsh % 1000) as f64 / 1000.0 - 0.5) * 2.0 * 24.0 }).collect()).collect();

    // ----- every marble's recording, as a function of the delays and leaps (so a swap can be tried before it is accepted)
    let ef = ((target + 3.4) * FPS) as usize + 2;
    let record = |delays: &Vec<Delay>, leaps: &Vec<Leap>, _q: &Vec<i32>| -> (Vec<f32>, Vec<u8>, Vec<f32>) {
        let mut pos = vec![0f32; ef * n * 2];
        let mut touch = vec![0u8; ef * n];
        let mut rank = vec![0f32; ef * n];
        // where marble `i` is when it is `ts` seconds into the reference path
        let base = |i: usize, ts: f64| -> (f64, f64) {
            if ts <= 0.0 { return (gate_x(i), start_pos.1); }
            let p = pref(ts.min(t_ref_end));
            let (mut x, y) = (p.0, p.1);
            // released from the gate row: slide across to the reference path while falling to the first ramp
            if ts < first_contact { x += (gate_x(i) - start_pos.0) * (1.0 - ts / first_contact); }
            // fan out in the air, back together on landing
            if let Some(k) = flights.iter().position(|fl| ts >= fl.0 && ts <= fl.1) {
                let u = (ts - flights[k].0) / (flights[k].1 - flights[k].0);
                x += fan[i][k] * (std::f64::consts::PI * u).sin();
            }
            // after the finish they drift to their own lanes instead of bunching up
            let post = smooth((ts - t_cross_ref) / 1.0);
            x += (gate_x(i) - x) * post;
            (x.clamp(RAD, W - RAD), y)
        };
        for f in 0..ef {
            let t = f as f64 / FPS;
            for i in 0..n {
                let leap = leaps.iter().find(|lp| (lp.a == i || lp.b == i) && t >= lp.t0 && t < lp.t1);
                let (x, y, ts, leaping);
                if let Some(lp) = leap {
                    let u = smooth((t - lp.t0) / (lp.t1 - lp.t0));
                    let (d_from, d_to) = if lp.a == i { (lp.d_a, lp.d_b) } else { (lp.d_b, lp.d_a) };
                    let (p0, p1) = (base(i, t - d_from), base(i, t - d_to));
                    x = p0.0 + (p1.0 - p0.0) * u;
                    y = p0.1 + (p1.1 - p0.1) * u - if lp.a == i { 46.0 * (std::f64::consts::PI * u).sin() } else { 0.0 };
                    ts = (t - d_from) + ((t - d_to) - (t - d_from)) * u;
                    leaping = true;
                } else {
                    ts = t - delays[i].at(t);
                    let p = base(i, ts);
                    x = p.0; y = p.1; leaping = false;
                }
                pos[(f * n + i) * 2] = x as f32; pos[(f * n + i) * 2 + 1] = y as f32;
                rank[f * n + i] = ts as f32;
                let tf = ((ts.max(0.0) * FPS).round() as usize).min(rf - 1);
                touch[f * n + i] = if ts > 0.0 && reference.touch[tf] == 1 && !leaping { 1 } else { 0 };
            }
        }
        (pos, touch, rank)
    };
    // closest two marbles come to each other between two frames, counting only pairs that involve marble `x` or `y`
    let closest = |pos: &Vec<f32>, from: usize, to: usize, x: usize, y: usize| -> f64 {
        let mut m = f64::MAX;
        for f in from..to.min(ef) { for a in 0..n { for b in (a + 1)..n {
            if a != x && a != y && b != x && b != y { continue; }
            let (ax, ay, bx, by) = (pos[(f * n + a) * 2] as f64, pos[(f * n + a) * 2 + 1] as f64, pos[(f * n + b) * 2] as f64, pos[(f * n + b) * 2 + 1] as f64);
            m = m.min((ax - bx).hypot(ay - by));
        } } }
        m
    };
    let mut delays: Vec<Delay> = (0..n).map(|i| Delay { start: slots[q[i] as usize], steps: vec![] }).collect();
    let mut leaps: Vec<Leap> = vec![];
    // when the reference marble hits something hard: a leap must not happen while either marble is landing
    let ref_hits: Vec<f64> = reference.events.iter().filter(|e| e.v >= 200.0 && e.kind != 3).map(|e| e.f as f64 / FPS).collect();
    let leap_len = 0.5;
    let busy = |m: usize, d: f64, t0: f64, t1: f64| ref_hits.iter().any(|h| { let x = h + d; let _ = m; x > t0 - 0.07 && x < t1 + 0.07 });
    let (mut dbg_windows, mut dbg_swaps, mut dbg_rejected) = (0, 0, 0);
    let mut tcur = 2.6;
    while tcur < target - 2.0 {
        let mut swap: Option<(usize, usize)> = None;
        if let Some(l) = plan_at(tcur) {
            if q[l] > 0 { if let Some(b) = (0..n).find(|&b| q[b] == q[l] - 1) { swap = Some((l, b)); } }
            else if n > 2 && rng.next() < 0.35 { let k = 1 + (rng.next() * (n as f64 - 2.0)) as i32; if let (Some(a), Some(b)) = ((0..n).find(|&a| q[a] == k), (0..n).find(|&b| q[b] == k + 1)) { swap = Some((b, a)); } }
        } else if n > 1 && rng.next() < 0.7 {
            let k = if rng.next() < 0.6 { 0 } else { (rng.next() * (n as f64 - 1.0)) as i32 };
            if let (Some(a), Some(b)) = ((0..n).find(|&a| q[a] == k), (0..n).find(|&b| q[b] == k + 1)) { swap = Some((b, a)); }
        }
        let Some((a, b)) = swap else { tcur += 1.4; continue; };
        // the first quiet moment in the next second and a half
        let (da, db) = (delays[a].at(tcur), delays[b].at(tcur));
        let mut t0 = tcur;
        let mut found = None;
        while t0 < tcur + 1.5 { if !busy(a, da, t0, t0 + leap_len) && !busy(b, db, t0, t0 + leap_len) { found = Some(t0); break; } t0 += 0.05; }
        dbg_windows += 1;
        let Some(t0) = found else { tcur += 0.9; continue; };
        let t1 = t0 + leap_len;
        let lp = Leap { t0, t1, a, b, d_a: da, d_b: db };
        // try it: refuse a swap that puts these two marbles closer to another marble than they were before
        leaps.push(lp);
        delays[a].steps.push((t1, db)); delays[b].steps.push((t1, da));
        let mut qt = q.clone(); qt.swap(a, b);
        let (try_pos, _, _) = record(&delays, &leaps, &qt);
        leaps.pop(); delays[a].steps.pop(); delays[b].steps.pop();
        let (base_pos, _, _) = record(&delays, &leaps, &q);
        let (f0, f1) = ((((t0 - 0.3) * FPS).max(0.0)) as usize, ((target - 0.2) * FPS) as usize);
        let (after, before) = (closest(&try_pos, f0, f1, a, b), closest(&base_pos, f0, f1, a, b));
        let fast = |pp: &Vec<f32>| -> f64 {
            let mut m = 0f64;
            for &i in &[a, b] { for f in (f0 + 1)..=((((t1 + 0.1) * FPS) as usize).min(ef - 1)) {
                let (x0, y0) = (pp[((f - 1) * n + i) * 2] as f64, pp[((f - 1) * n + i) * 2 + 1] as f64);
                let (x1, y1) = (pp[(f * n + i) * 2] as f64, pp[(f * n + i) * 2 + 1] as f64);
                m = m.max(((x1 - x0).hypot(y1 - y0)) * FPS);
            } }
            m
        };
        if fast(&try_pos) > 1400f64.max(fast(&base_pos) + 40.0) { dbg_rejected += 1; tcur += 0.6; continue; }
        if after < 16.0 && after < before - 2.0 { dbg_rejected += 1; tcur += 0.9; continue; }
        leaps.push(lp);
        delays[a].steps.push((t1, db)); delays[b].steps.push((t1, da));
        q.swap(a, b);
        dbg_swaps += 1;
        tcur = t1 + 0.8;
    }
    if std::env::var("MARBEAT_DEBUG_DESIGN").is_ok() { eprintln!("leap swaps: windows found {}, made {}, refused for crowding {}", dbg_windows, dbg_swaps, dbg_rejected); }
    let (mut pos, mut touch, rank) = record(&delays, &leaps, &q);
    let mut hop_events: Vec<Event> = vec![];
    // ----- every marble taps the track on its own groove while it rolls: small hops landing on the beat grid, each marble with its own pattern
    {
        const PAT: [&[f64]; 8] = [&[0.0, 1.0, 2.0, 3.0], &[0.0, 2.0], &[0.5, 1.5, 2.5, 3.5], &[0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5], &[0.0, 1.5, 2.0, 3.5], &[1.0, 3.0], &[0.0, 1.0, 1.5, 3.0], &[0.0, 0.5, 2.0, 2.5]];
        let busy = [0usize, 2, 4, 6, 7];
        let slow = [1usize, 5];
        let mut hits_at = vec![0u8; ef + 2];
        for ev in &reference.events { for i in 0..n { let t = ev.f as f64 / FPS + delays[i].at(ev.f as f64 / FPS); let fr = (t * FPS).round() as usize; if fr < ef { hits_at[fr] += 1; } } }
        let mut own: Vec<Vec<usize>> = vec![vec![]; n]; // frames of each marble's own impacts, to keep hops clear of landings
        for ev in &reference.events { for i in 0..n { let t = ev.f as f64 / FPS + delays[i].at(ev.f as f64 / FPS); let fr = (t * FPS).round() as usize; if fr < ef { own[i].push(fr); } } }
        let groups = cfg.beats.len() / 4;
        for g in 0..groups {
            let b0 = cfg.beats[g * 4];
            if b0 < 2.4 || b0 > target - 1.5 { continue; }
            let it = inten(cfg.bars, b0);
            // calm music: mostly slow grooves; loud music lets a few marbles play four-hit patterns
            let busy_n = if it < 0.3 { 0 } else if it < 0.7 { 2 } else { 3 };
            let mut who: Vec<usize> = (0..n).collect();
            for k in (1..who.len()).rev() { let j = (rng.next() * (k + 1) as f64) as usize; who.swap(k, j.min(k)); }
            for (rank, &m) in who.iter().enumerate() {
                let pat = if rank < busy_n { busy[(rng.next() * busy.len() as f64) as usize % busy.len()] } else { slow[(rng.next() * 2.0) as usize % 2] };
                for off in PAT[pat] {
                    let bi = g * 4 + off.floor() as usize;
                    let (ta, tb) = (cfg.beats[bi], cfg.beats.get(bi + 1).copied().unwrap_or(cfg.beats[bi] + period));
                    let t = ta + (tb - ta) * off.fract();
                    let fe = (t * FPS).round() as usize;
                    let fs = fe.saturating_sub(10);
                    if fe + 2 >= ef || fs < 2 { continue; }
                    // must be rolling the whole time, clear of its own landings, and never more than two marbles hitting at once
                    if (fs..=fe).filter(|f| touch[f * n + m] == 0).count() > 2 { continue; }
                    if own[m].iter().any(|&o| o + 6 > fs && o < fe + 8) { continue; }
                    if hits_at[fe] >= 2 { continue; }
                    let ty = pos[(fe * n + m) * 2 + 1] as f64;
                    if ty > finish_y - 90.0 { continue; }
                    let h = 5.0 + 6.0 * rng.next();
                    for f in fs..=fe { let u = (f - fs) as f64 / (fe - fs) as f64; pos[(f * n + m) * 2 + 1] -= (h * (std::f64::consts::PI * u).sin()) as f32; if f + 1 < fe { touch[f * n + m] = 0; } }
                    hits_at[fe] += 1; own[m].push(fe);
                    hop_events.push(Event { seg: u32::MAX, m: m as u8, f: fe as u32, kind: 1, v: 190.0, x: pos[(fe * n + m) * 2] });
                }
            }
        }
    }
    // ----- jostle: marbles that overlap shove each other apart and the shove fades, so a close pack really bumps
    let mut bumps = 0usize;
    {
        let mut off = vec![(0f64, 0f64); n];
        let (lo, hi) = (RAD, W - RAD);
        for f in 0..ef {
            for _ in 0..2 {
                for a in 0..n { for b in (a + 1)..n {
                    let pa = (pos[(f * n + a) * 2] as f64 + off[a].0, pos[(f * n + a) * 2 + 1] as f64 + off[a].1);
                    let pb = (pos[(f * n + b) * 2] as f64 + off[b].0, pos[(f * n + b) * 2 + 1] as f64 + off[b].1);
                    let (dx, dy) = (pa.0 - pb.0, pa.1 - pb.1);
                    let d = dx.hypot(dy).max(1e-6);
                    if d >= 2.0 * RAD { continue; }
                    let ov = 2.0 * RAD - d;
                    // mostly sideways: marbles rolling on a ramp are shoved off each other's line, not through the surface
                    let (nx, ny) = if dx.abs() < 1.0 && dy.abs() < 1.0 { (if a < b { -1.0 } else { 1.0 }, 0.0) } else { (dx / d, dy / d * 0.35) };
                    let nl = nx.hypot(ny);
                    let (nx, ny) = (nx / nl, ny / nl);
                    off[a].0 += nx * ov * 0.5; off[a].1 += ny * ov * 0.5;
                    off[b].0 -= nx * ov * 0.5; off[b].1 -= ny * ov * 0.5;
                    if ov > 3.0 { bumps += 1; }
                } }
            }
            for i in 0..n {
                let l = off[i].0.hypot(off[i].1);
                if l > 26.0 { off[i].0 *= 26.0 / l; off[i].1 *= 26.0 / l; }
                let x = (pos[(f * n + i) * 2] as f64 + off[i].0).clamp(lo, hi);
                pos[(f * n + i) * 2] = x as f32;
                pos[(f * n + i) * 2 + 1] += off[i].1 as f32;
                off[i].0 *= 0.86; off[i].1 *= 0.86;
            }
        }
    }
    if std::env::var("MARBEAT_DEBUG_DESIGN").is_ok() { let tc = touch.iter().filter(|t| **t == 1).count(); eprintln!("hops {}, bump frames {}, touching frames {}/{} (target {:.1}s)", hop_events.len(), bumps, tc, touch.len(), target); }
    // impacts: the reference marble's, shifted by each marble's delay (so they are on the half-beat grid too)
    let mut events: Vec<Event> = vec![];
    for ev in &reference.events {
        let te = ev.f as f64 / FPS;
        for i in 0..n {
            let mut t = te + delays[i].at(te);
            t = te + delays[i].at(t);
            let fr = (t * FPS).round() as usize;
            if fr < ef { events.push(Event { seg: ev.seg, m: i as u8, f: fr as u32, kind: ev.kind, v: ev.v, x: ev.x }); }
        }
    }
    events.extend(hop_events);
    events.sort_by_key(|e| e.f);
    // finish: who crosses first, and when it all ends
    let mut sim = Sim { n, frames: ef, pos, lead: vec![0; ef], cam: vec![0.0; ef], twin: 0.0, winner: 0, fins: vec![-1.0; n], events, touch, rank };
    let mut fins = vec![-1.0f64; n];
    let mut win: Option<(usize, usize)> = None;
    for f in 0..ef { for m in 0..n { if fins[m] < 0.0 && sim.at(f, m).1 > finish_y { fins[m] = f as f64 / FPS; if win.is_none() { win = Some((f, m)); } } } }
    let (wf, w) = win.unwrap_or((ef - 1, 0));
    let last_cross = fins.iter().cloned().fold(0.0f64, f64::max);
    let end = (((last_cross + 0.9) * FPS) as usize + 1).max(wf + (1.5 * FPS) as usize).min(ef);
    sim.pos.truncate(end * n * 2); sim.touch.truncate(end * n); sim.lead.truncate(end); sim.cam.truncate(end); sim.rank.truncate(end * n);
    sim.events.retain(|e| (e.f as usize) < end);
    let mut cam_y = 300.0f64;
    for f in 0..end {
        let mut best = (f64::MIN, 0usize);
        let mut ly = f64::MIN;
        for m in 0..n { let y = sim.at(f, m).1; ly = ly.max(y); let sc = if fins[m] >= 0.0 && fins[m] <= f as f64 / FPS { 1e9 - fins[m] } else { sim.rank[f * n + m] as f64 }; if sc > best.0 { best = (sc, m); } }
        sim.lead[f] = best.1 as u8;
        cam_y += (ly.min(finish_y - 80.0) - cam_y) * 0.1;
        sim.cam[f] = cam_y as f32;
    }
    sim.frames = end; sim.fins = fins; sim.winner = w; sim.twin = wf as f64 / FPS;
    relabel(&mut sim, cfg);
    // ----- scenery: rails and pegs filling the empty space so the whole thing reads as one track. They never touch a marble.
    {
        let nn = sim.n;
        let mut grid: std::collections::HashMap<(i32, i32), Vec<(f32, f32)>> = Default::default();
        for f in (0..sim.frames).step_by(2) { for m in 0..nn { let (x, y) = sim.at(f, m); grid.entry(((x / 40.0) as i32, (y / 40.0) as i32)).or_default().push((x as f32, y as f32)); } }
        let real: Vec<(f64, f64, f64, f64)> = course.segs.iter().map(|g| (g.x1, g.y1, g.x2, g.y2)).collect();
        let near_real = |x: f64, y: f64, rad: f64| -> bool { real.iter().any(|g| {
            let (dx, dy) = (g.2 - g.0, g.3 - g.1); let l2 = (dx * dx + dy * dy).max(1e-6);
            let u = (((x - g.0) * dx + (y - g.1) * dy) / l2).clamp(0.0, 1.0);
            (x - (g.0 + u * dx)).hypot(y - (g.1 + u * dy)) < rad }) };
        let clear = |x: f64, y: f64, rad: f64| -> bool {
            let (gx, gy) = ((x / 40.0) as i32, (y / 40.0) as i32);
            let reach = ((rad / 40.0).ceil() as i32) + 1;
            for dx in -reach..=reach { for dy in -reach..=reach { if let Some(v) = grid.get(&(gx + dx, gy + dy)) { for p in v { if ((p.0 as f64 - x).powi(2) + (p.1 as f64 - y).powi(2)).sqrt() < rad { return false; } } } } }
            !near_real(x, y, rad + 10.0)
        };
        let mut y = 150.0;
        let mut placed = 0;
        while y < finish_y + 100.0 && placed < 900 {
            for _ in 0..8 {
                let x = 24.0 + rng.next() * (W - 48.0);
                let yy = y + rng.next() * 60.0;
                if rng.next() < 0.35 {
                    if clear(x, yy, RAD + 5.0 + 12.0) { course.segs.push(Seg { x1: x, y1: yy, x2: x, y2: yy, r: 5.0, e: 0.9, kind: Kind::Peg, t0: 1e9, vis0: -1e9, t1: 1e9, shape: shape::PEG, owner: 255, kick: 0.0, bvx: 0.0 }); placed += 1; }
                } else {
                    // rails tilt in alternating directions row by row: a zig-zag track
                    let dir = if ((yy / 140.0) as i64) % 2 == 0 { 1.0 } else { -1.0 };
                    let ang = dir * (0.28 + 0.4 * rng.next());
                    let (dx, dy) = (ang.cos() * 30.0, ang.sin() * 30.0);
                    if clear(x, yy, RAD + 3.0 + 30.0 + 8.0) { course.segs.push(Seg { x1: x - dx, y1: yy - dy, x2: x + dx, y2: yy + dy, r: 3.0, e: 0.9, kind: Kind::Ramp, t0: 1e9, vis0: -1e9, t1: 1e9, shape: shape::PLANK, owner: 255, kick: 0.0, bvx: 0.0 }); placed += 1; }
                }
            }
            y += 38.0;
        }
        if std::env::var("MARBEAT_DEBUG_DESIGN").is_ok() { eprintln!("scenery pieces: {placed}"); }
    }
    Some(Designed { course, sim, landings, kinds })
}
