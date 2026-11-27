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
        let h = 330.0 * (0.62 + 0.95 * inten(cfg.bars, t)) * step;
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
    let mut chains: Vec<Chain> = vec![];
    for i in 0..n {
        let times = station_times(i, true);
        let (amp, freq, phase) = (7.0 + 13.0 * rng.next(), 0.22 + 0.5 * rng.next(), rng.next() * 6.28);
        let lane = 50.0 + total_named_lane * (i as f64 + 0.5);
        let half = (total_named_lane * 0.5 - 15.0).max(6.0);
        let mut c = Chain::default();
        let release = (times[0] - period).max(1.3);
        c.t.push(release); c.x.push(lane); c.y.push(y0); c.wall.push(false);
        let mut was_wall = false;
        for (k, t) in times.iter().enumerate() {
            let mut x = lane + (amp * (2.0 * std::f64::consts::PI * freq * t + phase).sin() + (rng.next() - 0.5) * 5.0).clamp(-half, half);
            let mut wall = false;
            // the outer marbles sometimes kick off the wall
            if (i == 0 || i == n - 1) && k > 0 && k + 1 < times.len() && !was_wall && rng.next() < 0.24 { x = if i == 0 { wall_l } else { wall_r }; wall = true; }
            was_wall = wall;
            c.t.push(*t); c.x.push(x.clamp(RAD + 1.0, W - RAD - 1.0)); c.y.push(depth(i, *t)); c.wall.push(wall);
        }
        // keep depths strictly increasing so every arc is a proper fall
        for k in 1..c.t.len() { if c.y[k] < c.y[k - 1] + 6.0 { c.y[k] = c.y[k - 1] + 6.0; } }
        arc(&mut c);
        chains.push(c);
    }
    // crowd: appears while the music is intense enough, sliding in from above and out again
    let mut crowd_present = vec![false; CROWD_MAX];
    for j in 0..ncrowd {
        let thr = 0.52 + 0.1 * j as f64;
        let times = station_times(n + j, false);
        let lane = 50.0 + (W - 100.0) * (j as f64 + 1.0) / (CROWD_MAX as f64 + 1.0);
        let off_j = 90.0 + rng.next() * 330.0;
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
                    let (half, th, off) = match shp { shape::TRAMP => (34.0, 3.5, 3.5), shape::DRUM => (26.0, 4.0, 4.0), _ => (28.0, 3.2, 3.0) };
                    let (cx, cy) = (px - nx * (RAD + off), py - ny * (RAD + off));
                    (Kind::Ramp, th, (cx - tx * half, cy - ty * half), (cx + tx * half, cy + ty * half))
                }
            };
            segs.push(Seg { x1: p1.0, y1: p1.1, x2: p2.0, y2: p2.1, r, e, kind, t0: c.t[k], vis0: -1e9, t1: 1e9, shape: shp, owner, kick: 0.0, bvx: 0.0 });
            events.push(Event { seg: u32::MAX, m: m as u8, f: (c.t[k] * fps).round() as u32, kind: if shp == shape::WALL { 2 } else { 1 }, v: dl as f32, x: px as f32 });
        }
    }
    // decoration: pegs and small planks that fill the gaps so the whole thing reads as one map. They never touch a marble:
    // every candidate is checked against every position any marble ever occupies.
    {
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
                let peg = rng.next() < 0.55;
                if peg {
                    if clear(x, yy, RAD + 5.0 + 9.0) { segs.push(Seg { x1: x, y1: yy, x2: x, y2: yy, r: 5.0, e: 0.9, kind: Kind::Peg, t0: 1e9, vis0: -1e9, t1: 1e9, shape: shape::PEG, owner: 255, kick: 0.0, bvx: 0.0 }); placed += 1; }
                } else {
                    let ang = (rng.next() - 0.5) * 1.1;
                    let (dx, dy) = (ang.cos() * 24.0, ang.sin() * 24.0);
                    if clear(x, yy, RAD + 3.0 + 24.0 + 6.0) { segs.push(Seg { x1: x - dx, y1: yy - dy, x2: x + dx, y2: yy + dy, r: 3.0, e: 0.9, kind: Kind::Ramp, t0: 1e9, vis0: -1e9, t1: 1e9, shape: shape::PLANK, owner: 255, kick: 0.0, bvx: 0.0 }); placed += 1; }
                }
            }
            y += 40.0;
        }
    }
    events.sort_by_key(|e| e.f);
    segs.push(Seg { x1: 0.0, y1: floor_y, x2: W, y2: floor_y, r: 6.0, e: 0.2, kind: Kind::Ramp, t0: -1e9, vis0: -1e9, t1: 1e9, shape: 0, owner: 255, kick: 0.0, bvx: 0.0 });
    let map = &MAPS[MAPS.iter().position(|m| m.key == cfg.map).unwrap_or(0)];
    let course = Course { segs, spins: vec![], bands: vec![vec![]], finish_y, floor_y, hc: finish_y, pal: palette_for(map.key, cfg.seed), map_key: map.key };
    let sim = Sim { n: total, frames, pos, lead, cam, twin, winner: w, fins, events, touch };
    Some((course, sim, total))
}
