//! Designed map mode: a real course of neon ramps and trampolines, composed from the music so the leader falls, lands, bounces and
//! rolls exactly on the beat.
//!
//! The map is built along the leader's path. It rolls down a ramp for N beats; at that beat the ramp ends under it and it falls;
//! the next ramp (or trampoline) is placed exactly where it will be one or two beats later, so the landing is on the beat. Trampolines
//! launch with a speed chosen so each bounce lasts exactly one beat, and end with a gap where the marble has drifted to. Everything
//! is static once built, so the whole map exists from the start. The rest of the field plays on the same ramps behind the leader
//! with real physics; rolling friction (never in the air) shapes who leads.
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

pub struct Designed { pub course: Course, pub sim: Sim, pub landings: Vec<f64>, pub heroes: Vec<usize>, pub segs: Vec<usize>, pub kinds: Vec<u8> }

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

fn choose(rng: &mut Rng, i: f64) -> Module {
    let r = rng.next();
    if i >= 0.62 && r < 0.5 { return Module::Bounce { m: 2 + (rng.next() * 3.0) as usize }; }
    let beats = if i < 0.35 { 2.0 + (rng.next() * 2.0).floor() } else if i < 0.7 { 1.0 + (rng.next() * 2.0).floor() } else { 1.0 };
    let fall = if i < 0.45 { 1.0 } else if rng.next() < 0.6 { 2.0 } else { 1.0 };
    Module::Roll { beats, fall }
}

/// who should lead, and the rolling friction that makes it so (friction only acts while touching the course)
fn apply_drag(e: &mut Engine, t: f64, cfg: &DesignCfg, _target: f64, _period: f64) {
    // each marble rolls a little differently, and that difference drifts, so the lead changes hands now and then
    for i in 0..cfg.n {
        let ph = (i as f64) * 1.7;
        e.roll_drag[i] = 0.0011 + 0.0009 * (0.5 + 0.5 * (0.55 * t + ph).sin()) + 0.0004 * (((i * 7919 + 13) % 17) as f64 / 17.0);
    }
}

/// The marbles are physically identical, so the planned winner and leaders are decided afterwards by choosing which marble's
/// trajectory carries which name: the winner's name goes to whoever wins, and each planned leader's name to whoever leads then.
fn relabel(sim: &mut Sim, cfg: &DesignCfg) {
    let n = cfg.n;
    let mut label_of: Vec<Option<usize>> = vec![None; n]; // trajectory -> name
    let mut used = vec![false; n];
    if let Some(w) = cfg.winner.filter(|w| *w < n) { label_of[sim.winner] = Some(w); used[w] = true; }
    for &(t, m) in cfg.plan {
        if m >= n || used[m] { continue; }
        let f = (((t + 2.5) * FPS) as usize).min(sim.frames - 1);
        let j = sim.lead[f] as usize;
        if label_of[j].is_none() { label_of[j] = Some(m); used[m] = true; }
    }
    let mut free: Vec<usize> = (0..n).filter(|m| !used[*m]).collect();
    let perm: Vec<usize> = (0..n).map(|j| match label_of[j] { Some(l) => l, None => free.remove(0) }).collect();
    let (nf, old_pos, old_touch) = (sim.frames, sim.pos.clone(), sim.touch.clone());
    for f in 0..nf { for j in 0..n { let l = perm[j];
        sim.pos[(f * n + l) * 2] = old_pos[(f * n + j) * 2]; sim.pos[(f * n + l) * 2 + 1] = old_pos[(f * n + j) * 2 + 1];
        sim.touch[f * n + l] = old_touch[f * n + j]; } }
    for f in 0..nf { sim.lead[f] = perm[sim.lead[f] as usize] as u8; }
    for e in sim.events.iter_mut() { if (e.m as usize) < n { e.m = perm[e.m as usize] as u8; } }
    let old_fins = sim.fins.clone();
    for j in 0..n { sim.fins[perm[j]] = old_fins[j]; }
    sim.winner = perm[sim.winner];
}

pub fn build(cfg: &DesignCfg) -> Option<Designed> {
    let n = cfg.n;
    let target = cfg.len - 2.5;
    if cfg.beats.len() < 8 { return None; }
    let period = { let mut v: Vec<f64> = cfg.beats.windows(2).map(|w| w[1] - w[0]).collect(); v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[v.len() / 2].max(0.25) };
    let nudge = hash_seed(&format!("{}|n", cfg.seed));
    let mut rng = Rng::new(hash_seed(&format!("{}|designed|{}", cfg.seed, cfg.map)));
    let kick = 0.5 * GRAV * period * 1.05; // a bounce launched at this speed lasts exactly one beat (calibrated for the engine's step)
    let mut course = Course::blank(cfg.map, cfg.seed, 900);
    // the opening ramp: everyone drops onto it and rolls right
    let r0 = course.add_seg(8.0, 250.0, W - 8.0, 250.0 + 0.16 * (W - 16.0), 5.0, 0.12, 0, 0.0, 0.0);
    let mut prev_dir = 1.0f64;
    let mut phase = Phase::Roll { seg: r0, cut_t: beat_at_or_after(cfg.beats, 2.4), fall: 1.0 };
    let mut landings: Vec<f64> = vec![];
    let mut heroes: Vec<usize> = vec![];
    let mut land_seg: Vec<usize> = vec![];
    let mut kinds: Vec<u8> = vec![]; // 0 = ramp landing, 1 = trampoline first landing, 2 = trampoline bounce
    let mut e = Engine::new(n, nudge, cfg.seed);
    let max_t = target + 3.6;
    let max_f = (max_t * FPS) as usize + 2;
    let mut finished_planning = false;
    let mut checks: Vec<(usize, usize, f64, f64, f64)> = vec![]; // (frame, hero, predicted x, y, land_t) for debugging
    let mut ghost_until: (usize, f64) = (usize::MAX, -1.0); // (marble, time)

    while e.f < max_f {
        let t = e.time();
        apply_drag(&mut e, t, cfg, target, period);
        if std::env::var("MARBEAT_DEBUG_DESIGN").is_ok() { if let Some(i) = checks.iter().position(|c| c.0 == e.f) { let c = checks.remove(i); let a = e.state(c.1); eprintln!("landing {:.2}: hero {} predicted ({:.0},{:.0}) actual ({:.0},{:.0}) v=({:.0},{:.0})", c.4, c.1, c.2, c.3, a.0, a.1, a.2, a.3); } }
        if ghost_until.0 != usize::MAX && t > ghost_until.1 { let g = ghost_until.0; e.ghost[g] = false; ghost_until = (usize::MAX, -1.0); }
        // work out the next module when the current one is about to end
        let wake = match phase {
            Phase::Roll { seg, cut_t, .. } => {
                let hero = (0..n).max_by(|&a, &b| e.state(a).1.partial_cmp(&e.state(b).1).unwrap()).unwrap();
                let (x, y, _, _, _) = e.state(hero);
                let g = &course.segs[seg];
                let (dx, dy) = (g.x2 - g.x1, g.y2 - g.y1);
                let l = dx.hypot(dy).max(1e-9);
                let remaining = l - ((x - g.x1) * dx + (y - g.y1) * dy) / l; // distance left along the ramp
                // at the beat, or sooner if the leader is about to run out of ramp (the landing is still placed on a beat)
                let (_, _, hvx, hvy, _) = e.state(hero);
                let downhill = (hvx * dx + hvy * dy) / l > 20.0;
                (t >= cut_t - 1e-9 && (downhill || t >= cut_t + 0.8)) || (remaining < 34.0 && e.touching(hero))
            }
            Phase::Bounce { last_t } => t >= last_t + 1.0 / FPS,
        };
        if wake && !finished_planning {
            let hero = (0..n).max_by(|&a, &b| e.state(a).1.partial_cmp(&e.state(b).1).unwrap()).unwrap();
            let (x, y, vx, vy, _) = e.state(hero);
            let (fall, land_floor);
            match phase {
                Phase::Roll { seg, fall: f, .. } => {
                    // the ramp ends under the leader at this beat
                    let g = &mut course.segs[seg];
                    let (dx, dy) = (g.x2 - g.x1, g.y2 - g.y1);
                    let l = dx.hypot(dy).max(1e-9);
                    // cut a marble-and-a-bit behind the leader so it is clear of the end and drops at once, and always leave a gap to the
                    // nearer wall that is wide enough to fall through (a marble pinned between the wall and the end cap never drops)
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
            let frames = ((land_t - t) * FPS).round().max(1.0) as usize;
            let (px, py, pvx_land, pvy_land) = predict(x, y, vx, vy, frames);
            landings.push(land_t);
            heroes.push(hero);
            kinds.push(0);
            checks.push(((land_t * FPS).round() as usize, hero, px, py, land_t));
            if ghost_until.0 != usize::MAX { let g = ghost_until.0; e.ghost[g] = false; }
            e.ghost[hero] = true; ghost_until = (hero, land_t + 0.05);
            let module = if last { Module::Roll { beats: 99.0, fall: 1.0 } } else { choose(&mut rng, inten(cfg.bars, land_t)) };
            match module {
                Module::Roll { beats, fall } => {
                    let alpha = 0.2 + 0.14 * rng.next();
                    let pref = if pvx_land.abs() > 80.0 { pvx_land.signum() } else { -prev_dir };
                    // pick the direction and roll length that leave room to fall without reaching a wall (a leader that hits a wall
                    // during its planned fall bounces back onto the ramp it just left)
                    let mut chosen: Option<(f64, f64, f64)> = None; // dir, beats, alpha
                    'search: for &dir in &[pref, -pref] {
                        for &al in &[alpha, alpha * 0.7] {
                            let (c, sn) = (al.cos(), al.sin());
                            let vt = pvx_land * dir * c + pvy_land * sn;
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
                    land_seg.push(seg);
                    phase = Phase::Roll { seg, cut_t: land_t + nb * period, fall };
                }
                Module::Bounce { m } => {
                    let ds = if px < W / 2.0 { 1.0 } else { -1.0 };
                    let bv = 105.0 + 55.0 * rng.next();
                    let mut m = m;
                    while m > 2 && (px + ds * (bv * (m - 1) as f64 * period + 10.0) > W - 30.0 || px + ds * (bv * (m - 1) as f64 * period + 10.0) < 30.0) { m -= 1; }
                    let reach = bv * (m - 1) as f64 * period + 10.0;
                    let sy = py + RAD + 5.0;
                    let ts = course.add_seg(px - ds * 70.0, sy, px + ds * reach, sy, 5.0, 0.1, shape::TRAMP, kick, ds * bv);
                    land_seg.push(ts);
                    *kinds.last_mut().unwrap() = 1;
                    for k in 1..m { landings.push(land_t + k as f64 * period); heroes.push(hero); kinds.push(2); }
                    phase = Phase::Bounce { last_t: land_t + (m - 1) as f64 * period };
                }
            }
            if last { finished_planning = true; }
        }
        if !e.step(&course, None) { break; }
    }
    let pass1 = e.finish(max_t);
    // the finish line sits where the leader is at the last beat, and a floor catches everyone after it
    let frame = ((target * FPS).round() as usize).min(pass1.frames - 1);
    let finish_y = (0..n).map(|i| pass1.at(frame, i).1).fold(f64::MIN, f64::max) - 4.0;
    course.finish_y = finish_y;
    course.floor_y = finish_y + 170.0;
    course.add_seg(0.0, finish_y + 170.0, W, finish_y + 170.0, 6.0, 0.2, 0, 0.0, 0.0);
    // finish the race from the recording itself (no second simulation): who crosses the line first, and when it all ends
    let mut sim = pass1;
    let nf = sim.frames;
    let mut fins = vec![-1.0f64; n];
    let mut win: Option<(usize, usize)> = None; // (frame, marble)
    for f in 0..nf { for m in 0..n { if fins[m] < 0.0 && sim.at(f, m).1 > finish_y { fins[m] = f as f64 / FPS; if win.is_none() { win = Some((f, m)); } } } }
    let (wf, w) = win.unwrap_or((nf - 1, 0));
    let end = (wf + (3.0 * FPS) as usize + 1).min(nf);
    sim.pos.truncate(end * n * 2); sim.touch.truncate(end * n); sim.lead.truncate(end); sim.cam.truncate(end);
    sim.events.retain(|e| (e.f as usize) < end);
    let mut cam_y = 300.0f64;
    for f in 0..end {
        let mut best = (f64::MIN, 0usize);
        let mut ly = f64::MIN;
        for m in 0..n { let y = sim.at(f, m).1; ly = ly.max(y); let sc = if fins[m] >= 0.0 && fins[m] <= f as f64 / FPS { 1e9 - fins[m] } else { y }; if sc > best.0 { best = (sc, m); } }
        sim.lead[f] = best.1 as u8;
        cam_y += (ly.min(finish_y - 80.0) - cam_y) * 0.1;
        sim.cam[f] = cam_y as f32;
    }
    sim.frames = end; sim.fins = fins; sim.winner = w; sim.twin = wf as f64 / FPS;
    relabel(&mut sim, cfg);
    Some(Designed { course, sim, landings, heroes, segs: land_seg, kinds })
}
