//! Beat mode: the race is choreographed from the music instead of simulated.
//! Every marble hits pads exactly on the beat grid and follows true gravity arcs between hits, so bounces and falls are locked to the
//! song. Each marble has its own groove (which beats and half-beats it hits, changing with the music's intensity), its own sideways
//! movement and fall speed, so they play against each other instead of in unison. Pads vary: planks, pegs, trampolines, flippers,
//! drums, wall kicks and a minority of orbs, each in its marble's colour. How far each marble has dropped decides the ranking, so lead
//! changes and the winner are fully controllable. Busy sections fall faster and bring extra "crowd" marbles into the pack.
use crate::course::*;
use crate::rng::{hash_seed, Rng};
use crate::sim::*;

pub struct BeatCfg<'a> {
    pub beats: &'a [f64],
    pub bars: &'a [(f64, f32)],
    pub n: usize,
    pub seed: &'a str,
    pub map: &'a str,
    pub len: f64,
    pub plan: &'a [(f64, usize)],
    pub winner: Option<usize>,
    pub crowd: usize,
    /// "race" look: one tight pack winding down a railed track, longer ramp pads, marbles shoving each other
    pub race: bool,
}

pub const CROWD_MAX: usize = 4;

fn inten(bars: &[(f64, f32)], t: f64) -> f64 {
    if bars.is_empty() { return 0.6; }
    let j = bars.partition_point(|b| b.0 <= t);
    bars[j.saturating_sub(1)].1 as f64
}

/// where in a 4-beat bar a marble hits (in beats)
const PATTERNS: [&[f64]; 8] = [
    &[0.0, 1.0, 2.0, 3.0],                         // every beat
    &[0.0, 2.0],                                   // half time
    &[0.5, 1.5, 2.5, 3.5],                         // off-beats
    &[0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5],     // eighth notes
    &[0.0, 1.5, 2.0, 3.5],                         // syncopated
    &[1.0, 3.0],                                   // backbeat
    &[0.0, 1.0, 1.5, 3.0],                         // pump
    &[0.0, 0.5, 2.0, 2.5],                         // pairs
];

/// Give every marble a different groove in this bar (shuffled from a set that suits the intensity) so they play against each other.
fn assign_patterns(rng: &mut Rng, i: f64, count: usize) -> Vec<usize> {
    // a budget of busy marbles per bar: calm music is mostly half-time and backbeat, loud music lets a few play four-hit patterns,
    // and only the loudest lets one marble play eighth notes. Everyone else keeps a slower groove, so there is always contrast.
    let (busy_budget, eighth_budget) = if i < 0.3 { (0, 0) } else if i < 0.7 { (2, 0) } else if i < 0.88 { (3, 0) } else { (3, 1) };
    let mut busy: Vec<usize> = vec![0, 2, 4, 6, 7]; // four-hit patterns
    let mut slow: Vec<usize> = vec![1, 5, 1, 5, 1]; // two-hit patterns
    for v in [&mut busy, &mut slow] { for k in (1..v.len()).rev() { let j = (rng.next() * (k + 1) as f64) as usize; v.swap(k, j.min(k)); } }
    let mut who: Vec<usize> = (0..count).collect();
    for k in (1..who.len()).rev() { let j = (rng.next() * (k + 1) as f64) as usize; who.swap(k, j.min(k)); }
    let mut out = vec![1usize; count];
    for (rank, m) in who.iter().enumerate() {
        out[*m] = if rank < eighth_budget { 3 } else if rank < eighth_budget + busy_budget { busy[rank % busy.len()] } else { slow[rank % slow.len()] };
    }
    out
}

#[derive(Clone, Default)]
struct Chain { t: Vec<f64>, x: Vec<f64>, y: Vec<f64>, vin: Vec<(f64, f64)>, vout: Vec<(f64, f64)>, wall: Vec<bool> }

fn arc(c: &mut Chain) {
    let k = c.t.len();
    c.vin = vec![(0.0, 0.0); k];
    c.vout = vec![(0.0, 0.0); k];
    for i in 0..k.saturating_sub(1) {
        let tt = c.t[i + 1] - c.t[i];
        let vx = (c.x[i + 1] - c.x[i]) / tt;
        let vy0 = (c.y[i + 1] - c.y[i]) / tt - 0.5 * GRAV * tt;
        c.vout[i] = (vx, vy0);
        c.vin[i + 1] = (vx, vy0 + GRAV * tt);
    }
    // after the last pad: straight down in the same lane
    if k > 0 { c.vout[k - 1] = (0.0, c.vin[k - 1].1); }
}

pub fn build(cfg: &BeatCfg) -> Option<(Course, Sim, usize)> {
    let n = cfg.n;
    let target = cfg.len - 2.5;
    let mut rng = Rng::new(hash_seed(&format!("{}|beatmode|{}", cfg.seed, cfg.map)));
    let period = { let mut v: Vec<f64> = cfg.beats.windows(2).map(|w| w[1] - w[0]).collect(); v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v.get(v.len() / 2).copied().unwrap_or(0.47).max(0.25) };
    // bars: (start, end, intensity)
    let mut bar_list: Vec<(f64, f64, f64)> = vec![];
    if cfg.bars.len() >= 3 {
        for (i, b) in cfg.bars.iter().enumerate() { let end = cfg.bars.get(i + 1).map(|x| x.0).unwrap_or(b.0 + 4.0 * period); bar_list.push((b.0, end, b.1 as f64)); }
    } else {
        let mut i = 0; while i + 4 < cfg.beats.len() { bar_list.push((cfg.beats[i], cfg.beats[i + 4], 0.6)); i += 4; }
    }
    bar_list.retain(|b| b.1 > 1.5 && b.0 < target);
    if bar_list.len() < 2 { return None; }

    // ----- how far each named marble has dropped at any time: rate follows the music, with each marble's own slow drift
    let step = 0.05;
    let t_start = 1.3;
    let y0 = 70.0;
    let nsteps = ((target + 1.5) / step) as usize + 2;
    let mut d = vec![vec![y0; nsteps]; n];
    let mut ar = vec![0.0f64; n];
    let plan_at = |t: f64| -> Option<usize> { cfg.plan.iter().filter(|p| p.0 <= t + 0.5 && p.1 < n).last().map(|p| p.1) };
    for s in 1..nsteps {
        let t = s as f64 * step;
        if t <= t_start { continue; }
        let h = 330.0 * if cfg.race { 0.8 } else { 1.0 } * (0.62 + 0.95 * inten(cfg.bars, t)) * step;
        let cur = (0..n).max_by(|&a, &b| d[a][s - 1].partial_cmp(&d[b][s - 1]).unwrap()).unwrap();
        let mut want = plan_at(t);
        let mut closing = false;
        if let Some(w) = cfg.winner { if t > target - 16.0 * period { want = Some(w); closing = true; } }
        for i in 0..n {
            ar[i] = 0.96 * ar[i] + (rng.next() - 0.5) * 0.13;
            let mut dd = ar[i];
            if let Some(l) = want {
                if cur != l {
                    let gap = d[cur][s - 1] - d[l][s - 1];
                    if i == l { dd += if closing { ((gap + 45.0) / 30.0 / h).clamp(0.3, 2.2) } else { ((gap + 30.0) / 50.0 / h).clamp(0.25, 1.2) }; } else if i == cur { dd -= if closing { 0.2 } else { 0.12 }; }
                } else if i == l { dd += 0.04; }
            }
            d[i][s] = d[i][s - 1] + h * (1.0 + dd.clamp(-0.35, if closing { 2.2 } else { 1.2 }));
        }
        // rubber band: nobody falls further than a screen-ish behind the leader, so the whole pack stays in view and in play
        let lead_d = (0..n).map(|i| d[i][s]).fold(f64::MIN, f64::max);
        let slack = if cfg.race { 170.0 } else { 400.0 };
        for i in 0..n { let behind = lead_d - d[i][s]; if behind > slack { d[i][s] += ((behind - slack) * 0.15).min(0.35 * h); } }
    }
    let depth = |i: usize, t: f64| -> f64 {
        let x = (t / step).max(0.0);
        let s = (x as usize).min(nsteps - 2);
        let f = (x - s as f64).clamp(0.0, 1.0);
        d[i][s] * (1.0 - f) + d[i][s + 1] * f
    };
    let lead_depth = |t: f64| (0..n).map(|i| depth(i, t)).fold(f64::MIN, f64::max);

    // ----- each marble's own groove, bar by bar
    let ncrowd = cfg.crowd.min(CROWD_MAX);
    let assign: Vec<Vec<usize>> = bar_list.iter().map(|b| assign_patterns(&mut rng, b.2, n + CROWD_MAX)).collect();
    let station_times = |who: usize, allow_double: bool| -> Vec<f64> {
        let mut times: Vec<f64> = vec![];
        for (bi, (b0, b1, _i)) in bar_list.iter().enumerate() {
            let pat = assign[bi][who];
            let p = (b1 - b0) / 4.0;
            for off in PATTERNS[pat] {
                let t = b0 + off * p;
                if t < 1.8 || t > target + 1e-6 { continue; }
                if !allow_double && (off * 2.0).fract() != 0.0 { continue; }
                if let Some(l) = times.last() { if t - l < 0.19 { continue; } }
                times.push(t);
            }
        }
        // everyone lands together on the final beat
        if times.last().map(|l| *l < target - 0.2).unwrap_or(true) { times.push(target); } else if let Some(l) = times.last_mut() { *l = target; }
        times
    };

    let total_named_lane = (W - 100.0) / n as f64;
    let wall_l = RAD + 1.0;
    let wall_r = W - RAD - 1.0;
    let (mut chains, mut rng_after) = (vec![], rng.clone());
    for (attempt, scale) in [1.0f64, 0.7, 0.45, 0.0].iter().enumerate() {
        let scale = *scale;
        let mut rng = rng.clone();
        let mut chains_try: Vec<Chain> = vec![];
        let mut worst_ok = true;
        {
            let chains = &mut chains_try;
        let mid = W / 2.0;
        let slot_of: Vec<usize> = { let mut v: Vec<usize> = (0..n).collect(); if cfg.race { for k in (1..n).rev() { let j = (rng.next() * (k + 1) as f64) as usize; v.swap(k, j.min(k)); } } v };
    let seed_ph = (hash_seed(&format!("{}|ph", cfg.seed)) % 628) as f64 / 100.0;
        let reach = (W / 2.0 - RAD - 26.0).max(40.0);
        for i in 0..n {
            let times = station_times(i, true);
            // every marble wanders across the whole track on its own slow path, so the pack mixes and they cross over each other
            let (amp, freq, phase) = (scale * (0.55 + 0.4 * rng.next()), 0.05 + 0.07 * rng.next(), rng.next() * 6.28);
            let wob = (4.0 + 6.0 * rng.next(), 0.5 + 0.9 * rng.next(), rng.next() * 6.28);
            let lane = 50.0 + total_named_lane * (i as f64 + 0.5);
            let mut c = Chain::default();
            let release = (times[0] - period).max(1.3);
            c.t.push(release); c.x.push(lane); c.y.push(y0); c.wall.push(false);
            let mut was_wall = false;
            for (k, t) in times.iter().enumerate() {
                let last = k + 1 == times.len();
                // the wander fades in over the first seconds from the starting lane, and everyone lines up again for the finish
                let fade = ((*t - release - 2.0) / 6.0).clamp(0.0, 1.0);
                let w = if scale == 0.0 { lane } else if cfg.race {
                // the whole pack winds down one track (a slow switchback) and every marble keeps its own slot across it,
                // weaving a little inside that slot. Marbles pass each other by running ahead or behind, so they never need to cross sideways.
                let sp = (((W - 150.0) / n as f64).min(46.0)).max(30.0);
                let centre_amp = (W / 2.0 - 34.0 - (n as f64 - 1.0) / 2.0 * sp).max(18.0);
                let centre = mid + centre_amp * (2.0 * std::f64::consts::PI * *t / 11.0 + seed_ph).sin();
                let slot = (slot_of[i] as f64 - (n as f64 - 1.0) / 2.0) * sp;
                let a = ((sp - 2.0 * RAD - 3.0) / 2.0 * 0.85).max(0.0);
                centre + slot + a * (2.0 * std::f64::consts::PI * (0.15 + 1.2 * freq) * t + phase).sin()
            } else { mid + reach * amp * (2.0 * std::f64::consts::PI * freq * t + phase).sin() + wob.0 * (wob.1 * t + wob.2).sin() };
                let mut x = if cfg.race {
                // the pack spreads back out into the finishing lanes over the last few seconds instead of jumping there
                let b = { let u = ((*t - (target - 4.0)) / 4.0).clamp(0.0, 1.0); u * u * (3.0 - 2.0 * u) };
                let pack = lane + (w - lane) * fade;
                pack + (lane - pack) * b
            } else if last { lane } else { lane + (w - lane) * fade };
                let mut wall = false;
                // out by a wall, a marble sometimes kicks off it
                if !cfg.race && !last && k > 0 && !was_wall && (x < wall_l + 70.0 || x > wall_r - 70.0) && rng.next() < 0.6 { x = if x < mid { wall_l } else { wall_r }; wall = true; }
                was_wall = wall;
                c.t.push(*t); c.x.push(x.clamp(RAD + 1.0, W - RAD - 1.0)); c.y.push(depth(i, *t)); c.wall.push(wall);
            }
            // keep depths strictly increasing so every arc is a proper fall
            for k in 1..c.t.len() { if c.y[k] < c.y[k - 1] + 6.0 { c.y[k] = c.y[k - 1] + 6.0; } }
            chains.push(c);
        }
        // marbles that would touch are nudged apart sideways at their nearest hits until nobody overlaps
        {
            let at = |c: &Chain, t: f64| -> (f64, f64) {
                let kk = c.t.len();
                if t <= c.t[0] { return (c.x[0], c.y[0]); }
                if t >= c.t[kk - 1] { return (c.x[kk - 1], c.y[kk - 1] + c.vout[kk - 1].1 * (t - c.t[kk - 1])); }
                let k = c.t.partition_point(|x| *x <= t).saturating_sub(1).min(kk - 2);
                let s = t - c.t[k];
                (c.x[k] + c.vout[k].0 * s, c.y[k] + c.vout[k].1 * s + 0.5 * GRAV * s * s)
            };
            let need = if cfg.race { 2.0 * RAD + 3.0 } else { 2.0 * RAD + 7.0 };
            for _ in 0..120 {
                for c in chains.iter_mut() { arc(c); }
                let mut moved = false; worst_ok = true;
                let mut tt = 1.3;
                while tt < target + 0.4 {
                    for a in 0..n { for b in (a + 1)..n {
                        let (pa, pb) = (at(&chains[a], tt), at(&chains[b], tt));
                        let dist = (pa.0 - pb.0).hypot(pa.1 - pb.1);
                        if dist >= need { continue; }
                        if dist < need - 1.5 { worst_ok = false; }
                        let sgn = if pa.0 > pb.0 { 1.0 } else if pa.0 < pb.0 { -1.0 } else if a < b { -1.0 } else { 1.0 };
                        let push = (need - dist) * 0.5 + 4.0;
                        for (m, dir) in [(a, sgn), (b, -sgn)] {
                            let c = &mut chains[m];
                            // the two hits either side of this moment both move, so the arc between them shifts as a whole
                            let k = c.t.partition_point(|x| *x <= tt).max(1).min(c.t.len() - 1);
                            for kk in [k.saturating_sub(1).max(1), k] {
                                if c.wall[kk] { continue; }
                                c.x[kk] = (c.x[kk] + dir * push).clamp(RAD + 1.0, W - RAD - 1.0);
                                moved = true;
                            }
                        }
                    } }
                    tt += 1.0 / 30.0;
                }
                if !moved || worst_ok { if !moved { worst_ok = true; } break; }
            }
            for c in chains.iter_mut() { arc(c); }
            // an arc faster than a natural fall means the nudging got too violent: try again with less wander
            let fast = chains.iter().any(|c| c.vin.iter().chain(c.vout.iter()).any(|v| v.0.hypot(v.1) > 1420.0 || v.0.abs() > 650.0));
            if std::env::var("MARBEAT_DEBUG_BEAT").is_ok() { let mx = chains.iter().flat_map(|c| c.vout.iter()).map(|v| v.0.abs()).fold(0.0f64, f64::max); let my = chains.iter().flat_map(|c| c.vout.iter().chain(c.vin.iter())).map(|v| v.0.hypot(v.1)).fold(0.0f64, f64::max); eprintln!("  attempt {attempt} scale {scale}: gaps_ok={worst_ok} max|vx|={mx:.0} max speed={my:.0}"); }
            if fast { worst_ok = false; }
        }
        }
        // the last, lane-only attempt is accepted whatever happens: lanes can't overlap
        if worst_ok || attempt == 3 { if std::env::var("MARBEAT_DEBUG_BEAT").is_ok() { eprintln!("beatmode: attempt {} ok={}", attempt, worst_ok); } chains = chains_try; rng_after = rng; break; }
    }
    let mut rng = rng_after;

    // crowd: appears while the music is intense enough, sliding in from above and out again
    let mut crowd_present = vec![false; CROWD_MAX];
    for j in 0..ncrowd {
        let thr = 0.52 + 0.1 * j as f64;
        let times = station_times(n + j, false);
        let lane = 50.0 + (W - 100.0) * (j as f64 + 1.0) / (CROWD_MAX as f64 + 1.0);
        let off_j = 380.0 + rng.next() * 220.0; // behind the named pack
        let mut c = Chain::default();
        let mut pres = 0.0f64;
        let (amp, phase) = (6.0 + 6.0 * rng.next(), rng.next() * 6.28);
        let release = (times[0] - period).max(1.3);
        c.t.push(release); c.x.push(lane); c.y.push(lead_depth(release) - off_j - 1100.0); c.wall.push(false);
        for t in &times {
            let want = if inten(cfg.bars, *t) >= thr { 1.0 } else { 0.0 };
            pres += (want - pres).clamp(-0.34, 0.34);
            if pres > 0.0 { crowd_present[j] = true; }
            c.t.push(*t); c.x.push(lane + amp * (1.1 * t + phase).sin()); c.y.push(lead_depth(*t) - off_j - (1.0 - pres) * 1100.0); c.wall.push(false);
        }
        for k in 1..c.t.len() { if c.y[k] < c.y[k - 1] + 6.0 { c.y[k] = c.y[k - 1] + 6.0; } }
        arc(&mut c);
        chains.push(c);
    }
    let total = chains.len();

    // ----- finish line and per-frame sampling
    let finish_y = (0..n).map(|i| depth(i, target)).fold(f64::MIN, f64::max) - 4.0;
    let floor_y = finish_y + 170.0;
    let fps = FPS;
    let max_t = cfg.len * 1.6;
    let max_f = (max_t * fps) as usize + 2;
    let mut pos = vec![0f32; max_f * total * 2];
    let mut touch = vec![0u8; max_f * total];
    let mut lead = vec![0u8; max_f];
    let mut cam = vec![0f32; max_f];
    let mut fins = vec![-1.0f64; total];
    let mut ptr = vec![0usize; total];
    let mut cam_y = 300.0f64;
    let (mut winner, mut win_f, mut frames) = (-1i64, -1i64, max_f);
    for f in 0..max_f {
        let t = f as f64 / fps;
        let mut named_y = vec![0f64; n];
        for m in 0..total {
            let c = &chains[m];
            let kk = c.t.len();
            let (x, y);
            if t <= c.t[0] { x = c.x[0]; y = c.y[0]; }
            else if t >= c.t[kk - 1] {
                let s = t - c.t[kk - 1];
                let (_, vy) = c.vout[kk - 1];
                let tc = ((1300.0 - vy) / GRAV).max(0.0); // terminal velocity like the physics mode
                x = c.x[kk - 1];
                y = if s < tc { c.y[kk - 1] + vy * s + 0.5 * GRAV * s * s } else { c.y[kk - 1] + vy * tc + 0.5 * GRAV * tc * tc + 1300.0 * (s - tc) };
            } else {
                while ptr[m] + 1 < kk - 1 && c.t[ptr[m] + 1] <= t { ptr[m] += 1; }
                let k = ptr[m];
                let s = t - c.t[k];
                let (vx, vy) = c.vout[k];
                x = c.x[k] + vx * s; y = c.y[k] + vy * s + 0.5 * GRAV * s * s;
            }
            pos[(f * total + m) * 2] = x as f32; pos[(f * total + m) * 2 + 1] = y as f32;
            if m < n { named_y[m] = y; }
            if c.t.iter().skip(1).any(|st| (st - t).abs() < 2.5 / fps) { touch[f * total + m] = 1; }
            if m < n && fins[m] < 0.0 && y > finish_y { fins[m] = t; if winner < 0 { winner = m as i64; win_f = f as i64; } }
        }
        let l = (0..n).max_by(|&a, &b| { let sa = if fins[a] >= 0.0 { 1e9 - fins[a] } else { named_y[a] }; let sb = if fins[b] >= 0.0 { 1e9 - fins[b] } else { named_y[b] }; sa.partial_cmp(&sb).unwrap() }).unwrap();
        lead[f] = l as u8;
        let ly = named_y.iter().cloned().fold(f64::MIN, f64::max);
        cam_y += (ly.min(finish_y - 80.0) - cam_y) * 0.1;
        cam[f] = cam_y as f32;
        if win_f >= 0 && f as i64 >= win_f + (3.0 * fps) as i64 { frames = f + 1; break; }
    }
    pos.truncate(frames * total * 2); touch.truncate(frames * total); lead.truncate(frames); cam.truncate(frames);
    let w = if winner >= 0 { winner as usize } else { lead[frames - 1] as usize };
    let twin = if fins[w] >= 0.0 { fins[w] } else { max_t };

    // ----- the pads: a different kind of surface for each hit, shaped by the bounce it has to produce
    let mut segs: Vec<Seg> = vec![];
    let mut events: Vec<Event> = vec![];
    for (m, c) in chains.iter().enumerate() {
        if m >= n && !crowd_present[m - n] { continue; }
        let mut last_shape = 0u8;
        for k in 1..c.t.len() - 1 {
            let (dvx, dvy) = (c.vout[k].0 - c.vin[k].0, c.vout[k].1 - c.vin[k].1);
            let dl = dvx.hypot(dvy);
            if dl < 60.0 { continue; }
            let (nx, ny) = (dvx / dl, dvy / dl);
            let (px, py) = (c.x[k], c.y[k]);
            let (tx, ty) = (-ny, nx);
            let crowd = m >= n;
            let shp = if c.wall[k] { shape::WALL } else {
                let up = ny < -0.7;
                let side = nx.abs() > 0.6;
                let mut opts: Vec<(u8, f64)> = if up { vec![(shape::TRAMP, 0.34), (shape::DRUM, 0.26), (shape::PLANK, 0.24), (shape::ORB, 0.16)] }
                    else if side { vec![(shape::FLIPPER, 0.34), (shape::PLANK, 0.26), (shape::PEG, 0.22), (shape::ORB, 0.18)] }
                    else { vec![(shape::PLANK, 0.34), (shape::PEG, 0.24), (shape::FLIPPER, 0.2), (shape::ORB, 0.22)] };
                if cfg.race { for o in opts.iter_mut() { if o.0 == shape::ORB || o.0 == shape::PEG { o.1 *= 0.3; } } }
                for o in opts.iter_mut() { if o.0 == last_shape { o.1 *= 0.25; } } // never the same pad twice in a row if avoidable
                let tot: f64 = opts.iter().map(|o| o.1).sum();
                let mut r = rng.next() * tot;
                let mut pick = opts[0].0;
                for o in &opts { r -= o.1; if r <= 0.0 { pick = o.0; break; } }
                pick
            };
            last_shape = shp;
            let owner = if crowd { 255 } else { m as u8 };
            let e = if crowd { 0.4 } else { 0.9 };
            let (kind, r, p1, p2) = match shp {
                shape::ORB => { let rb = if crowd { 11.0 } else { 15.0 }; let (cx, cy) = (px - nx * (RAD + rb), py - ny * (RAD + rb)); (Kind::Bump, rb, (cx, cy), (cx, cy)) }
                shape::PEG => { let rb = 5.5; let (cx, cy) = (px - nx * (RAD + rb), py - ny * (RAD + rb)); (Kind::Peg, rb, (cx, cy), (cx, cy)) }
                shape::WALL => (Kind::Peg, 28.0, (px, py), (px, py)),
                shape::FLIPPER => {
                    let tip = (px - nx * (RAD + 3.0), py - ny * (RAD + 3.0));
                    let a = (tip.0 + tx * 50.0, tip.1 + ty * 50.0);
                    let b = (tip.0 - tx * 50.0, tip.1 - ty * 50.0);
                    (Kind::Ramp, 4.0, if a.1 > b.1 { a } else { b }, tip) // pivot hangs lower than the tip
                }
                _ => {
                    let (half, th, off) = match shp { shape::TRAMP => (34.0, 3.5, 3.5), shape::DRUM => (26.0, 4.0, 4.0), _ => (if cfg.race { 42.0 } else { 28.0 }, 3.2, 3.0) };
                    let (cx, cy) = (px - nx * (RAD + off), py - ny * (RAD + off));
                    (Kind::Ramp, th, (cx - tx * half, cy - ty * half), (cx + tx * half, cy + ty * half))
                }
            };
            segs.push(Seg { x1: p1.0, y1: p1.1, x2: p2.0, y2: p2.1, r, e, kind, t0: c.t[k], vis0: -1e9, t1: 1e9, shape: shp, owner, kick: 0.0, bvx: 0.0 });
            events.push(Event { seg: u32::MAX, m: m as u8, f: (c.t[k] * fps).round() as u32, kind: if shp == shape::WALL { 2 } else { 1 }, v: dl as f32, x: px as f32 });
        }
    }
    // decoration (lane look): pegs and small planks that fill the gaps so the whole thing reads as one map. They never touch a marble:
    // every candidate is checked against every position any marble ever occupies.
    if !cfg.race {
        let mut pts: Vec<(f32, f32)> = vec![];
        for f in (0..frames).step_by(2) { for m in 0..total { if m < n || crowd_present.get(m - n).copied().unwrap_or(false) { pts.push((pos[(f * total + m) * 2], pos[(f * total + m) * 2 + 1])); } } }
        let pads: Vec<(f64, f64)> = segs.iter().map(|g| ((g.x1 + g.x2) / 2.0, (g.y1 + g.y2) / 2.0)).collect();
        let mut grid: std::collections::HashMap<(i32, i32), Vec<(f32, f32)>> = Default::default();
        for p in &pts { grid.entry(((p.0 / 40.0) as i32, (p.1 / 40.0) as i32)).or_default().push(*p); }
        let clear = |x: f64, y: f64, rad: f64| -> bool {
            let (gx, gy) = ((x / 40.0) as i32, (y / 40.0) as i32);
            let reach = ((rad / 40.0).ceil() as i32) + 1;
            for dx in -reach..=reach { for dy in -reach..=reach { if let Some(v) = grid.get(&(gx + dx, gy + dy)) { for p in v { if ((p.0 as f64 - x).powi(2) + (p.1 as f64 - y).powi(2)).sqrt() < rad { return false; } } } } }
            !pads.iter().any(|p| (p.0 - x).hypot(p.1 - y) < 46.0)
        };
        let mut y = 160.0;
        let mut placed = 0;
        while y < finish_y + 120.0 && placed < 1100 {
            for _ in 0..9 {
                let x = 28.0 + rng.next() * (W - 56.0);
                let yy = y + rng.next() * 70.0;
                let peg = rng.next() < 0.4;
                if peg {
                    if clear(x, yy, RAD + 5.0 + 9.0) { segs.push(Seg { x1: x, y1: yy, x2: x, y2: yy, r: 5.0, e: 0.9, kind: Kind::Peg, t0: 1e9, vis0: -1e9, t1: 1e9, shape: shape::PEG, owner: 255, kick: 0.0, bvx: 0.0 }); placed += 1; }
                } else {
                    // rails tilt in alternating directions row by row, so the decoration reads as a zig-zag track
                    let dir = if ((yy / 140.0) as i64) % 2 == 0 { 1.0 } else { -1.0 };
                    let ang = dir * (0.28 + 0.4 * rng.next());
                    let (dx, dy) = (ang.cos() * 30.0, ang.sin() * 30.0);
                    if clear(x, yy, RAD + 3.0 + 30.0 + 6.0) { segs.push(Seg { x1: x - dx, y1: yy - dy, x2: x + dx, y2: yy + dy, r: 3.0, e: 0.9, kind: Kind::Ramp, t0: 1e9, vis0: -1e9, t1: 1e9, shape: shape::PLANK, owner: 255, kick: 0.0, bvx: 0.0 }); placed += 1; }
                }
            }
            y += 40.0;
        }
    }
    if cfg.race {
        // the track: a railed channel that follows the pack down the course, with a few pegs outside it
        let stepf = 6usize;
        let samples: Vec<(f64, f64, f64)> = (0..frames).step_by(stepf).map(|f| {
            let xs: Vec<f64> = (0..n).map(|m| pos[(f * total + m) * 2] as f64).collect();
            let ys: Vec<f64> = (0..n).map(|m| pos[(f * total + m) * 2 + 1] as f64).collect();
            (xs.iter().sum::<f64>() / n as f64, ys.iter().sum::<f64>() / n as f64, 0.0)
        }).collect();
        let k = samples.len();
        let mut hw = vec![0f64; k];
        for (si, f) in (0..frames).step_by(stepf).enumerate() {
            // half-width: how far any marble strays from the pack's middle within about a second either side, plus room
            let mut w = 0.0f64;
            for ff in f.saturating_sub(40)..(f + 40).min(frames) { let c = samples[(ff / stepf).min(k - 1)].0; for m in 0..n { w = w.max((pos[(ff * total + m) * 2] as f64 - c).abs()); } }
            hw[si] = (w + RAD + 52.0).max(78.0);
        }
        for si in 0..k.saturating_sub(1) {
            for side in [-1.0f64, 1.0] {
                let (x1, x2) = ((samples[si].0 + side * hw[si]).clamp(8.0, W - 8.0), (samples[si + 1].0 + side * hw[si + 1]).clamp(8.0, W - 8.0));
                segs.push(Seg { x1, y1: samples[si].1, x2, y2: samples[si + 1].1, r: 7.0, e: 0.9, kind: Kind::Ramp, t0: 1e9, vis0: -1e9, t1: 1e9, shape: shape::PLANK, owner: 255, kick: 0.0, bvx: 0.0 });
            }
        }
        // a few pegs in the margins outside the rails
        let mut yy = 200.0;
        while yy < finish_y + 80.0 {
            let si = ((yy / (finish_y + 1.0)) * k as f64) as usize; let _ = si;
            for _ in 0..2 { let x = 14.0 + rng.next() * (W - 28.0);
                let near_rail = samples.iter().enumerate().any(|(i, s)| (s.1 - yy).abs() < 160.0 && (x - s.0).abs() < hw[i] + 24.0);
                if !near_rail { segs.push(Seg { x1: x, y1: yy, x2: x, y2: yy, r: 5.0, e: 0.9, kind: Kind::Peg, t0: 1e9, vis0: -1e9, t1: 1e9, shape: shape::PEG, owner: 255, kick: 0.0, bvx: 0.0 }); } }
            yy += 70.0;
        }
        // shoving: marbles that touch push each other aside and the shove fades, so the pack really jostles
        let mut off = vec![(0f64, 0f64); n];
        let mut prev = vec![(0f64, 0f64); n];
        for f in 0..frames {
            for _ in 0..5 { for a in 0..n { for b in (a + 1)..n {
                let pa = (pos[(f * total + a) * 2] as f64 + off[a].0, pos[(f * total + a) * 2 + 1] as f64 + off[a].1);
                let pb = (pos[(f * total + b) * 2] as f64 + off[b].0, pos[(f * total + b) * 2 + 1] as f64 + off[b].1);
                let (dx, dy) = (pa.0 - pb.0, pa.1 - pb.1);
                let d = dx.hypot(dy).max(1e-6);
                let reach = 2.0 * RAD + 6.0;
                if d >= reach { continue; }
                let ov = reach - d;
                let (nx, ny) = if dx.abs() < 1.0 && dy.abs() < 1.0 { (if a < b { -1.0 } else { 1.0 }, 0.0) } else { (dx / d, dy / d * 0.5) };
                let nl = nx.hypot(ny); let (nx, ny) = (nx / nl, ny / nl);
                // a marble pinned against a wall can't give way, so the other one takes the whole shove
                let pinned = |p: f64, dir: f64| (p >= W - RAD - 2.0 && dir > 0.0) || (p <= RAD + 2.0 && dir < 0.0);
                let (sa, sb) = if pinned(pa.0, nx) { (0.0, 1.0) } else if pinned(pb.0, -nx) { (1.0, 0.0) } else { (0.5, 0.5) };
                off[a].0 += nx * ov * sa; off[a].1 += ny * ov * sa; off[b].0 -= nx * ov * sb; off[b].1 -= ny * ov * sb;
            } } }
            for i in 0..n {
                // a shove builds up over a few frames instead of snapping, so nothing speeds up unnaturally
                let (dx, dy) = (off[i].0 - prev[i].0, off[i].1 - prev[i].1); let dl = dx.hypot(dy);
                if dl > 8.0 { off[i] = (prev[i].0 + dx * 8.0 / dl, prev[i].1 + dy * 8.0 / dl); }
                let l = off[i].0.hypot(off[i].1); if l > 30.0 { off[i].0 *= 30.0 / l; off[i].1 *= 30.0 / l; }
                pos[(f * total + i) * 2] = (pos[(f * total + i) * 2] as f64 + off[i].0).clamp(RAD, W - RAD) as f32;
                pos[(f * total + i) * 2 + 1] += off[i].1 as f32;
                off[i].0 *= 0.84; off[i].1 *= 0.84;
                prev[i] = off[i];
            }
        }
    }
    events.sort_by_key(|e| e.f);
    segs.push(Seg { x1: 0.0, y1: floor_y, x2: W, y2: floor_y, r: 6.0, e: 0.2, kind: Kind::Ramp, t0: -1e9, vis0: -1e9, t1: 1e9, shape: 0, owner: 255, kick: 0.0, bvx: 0.0 });
    let map = &MAPS[MAPS.iter().position(|m| m.key == cfg.map).unwrap_or(0)];
    let course = Course { segs, spins: vec![], bands: vec![vec![]], finish_y, floor_y, hc: finish_y, pal: palette_for(map.key, cfg.seed), map_key: map.key };
    let sim = Sim { n: total, frames, pos, lead, cam, twin, winner: w, fins, events, touch, rank: vec![] };
    Some((course, sim, total))
}
