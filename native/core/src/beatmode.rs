//! Beat mode: the race is choreographed from the music instead of simulated.
//! Every marble hits a pad exactly on a beat (half-beats in intense parts), then follows a true gravity arc to the next pad one
//! beat later, so bounces and falls are locked to the song. How far each marble drops per beat decides the ranking, which makes
//! lead changes and the winner fully controllable. Busy sections fall faster and bring extra "crowd" marbles into the pack.
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
}

pub const CROWD_MAX: usize = 4;

fn inten(bars: &[(f64, f32)], t: f64) -> f64 {
    if bars.is_empty() { return 0.6; }
    let j = bars.partition_point(|b| b.0 <= t);
    bars[j.saturating_sub(1)].1 as f64
}

/// one marble's stations: time, contact point, and the velocity it arrives with / leaves with
#[derive(Clone, Default)]
struct Chain { t: Vec<f64>, x: Vec<f64>, y: Vec<f64>, vin: Vec<(f64, f64)>, vout: Vec<(f64, f64)> }

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
    // stations: every beat from the first one after 1.8 s, plus half-beats while the music is intense
    let mut times: Vec<f64> = vec![];
    for (k, b) in cfg.beats.iter().enumerate() {
        if *b < 1.8 || *b > target { continue; }
        times.push(*b);
        if let Some(nx) = cfg.beats.get(k + 1) { let mid = (b + nx) * 0.5; if inten(cfg.bars, mid) > 0.78 && mid < target && k % 2 == 0 { times.push(mid); } }
    }
    if times.len() < 6 { return None; }
    let period = cfg.beats.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max).max(0.3);
    let start_t = times[0] - period; // the marbles are released one beat before the first hit
    times.insert(0, start_t);
    let kk = times.len();
    let mut rng = Rng::new(hash_seed(&format!("{}|beatmode|{}", cfg.seed, cfg.map)));
    let total_lanes = n + CROWD_MAX;
    let lane_w = (W - 100.0) / total_lanes as f64;
    let lane_x = |lane: usize| 50.0 + lane_w * (lane as f64 + 0.5);
    // named marbles spread across the lanes, crowd fills the gaps
    let named_lane: Vec<usize> = (0..n).map(|i| (((i as f64 + 0.5) * total_lanes as f64 / n as f64) - 0.5).round() as usize).collect();
    let free_lanes: Vec<usize> = (0..total_lanes).filter(|l| !named_lane.contains(l)).collect();
    // shared zig-zag so neighbours never collide: every marble swings the same way on the same beat
    let mut swing: Vec<f64> = vec![0.0; kk];
    let mut sgn = 1.0;
    for k in 0..kk { let i = inten(cfg.bars, times[k]); swing[k] = sgn * (6.0 + 9.0 * i + rng.next() * 3.0); sgn = -sgn; }
    let wl = lane_w * 0.5 - 8.0;
    // ----- ranking: how far each named marble has dropped at every station
    let y0 = 70.0;
    let mut depth = vec![vec![y0; kk]; n];
    let mut ar = vec![0.0f64; n];
    let plan_at = |t: f64| -> Option<usize> { cfg.plan.iter().filter(|p| p.0 <= t + 0.5 && p.1 < n).last().map(|p| p.1) };
    for k in 0..kk - 1 {
        let tt = times[k + 1] - times[k];
        let tau = times[k];
        let rate = 330.0 * (0.62 + 0.95 * inten(cfg.bars, tau)); // busy music falls faster
        let h = rate * tt;
        let cur = (0..n).max_by(|&a, &b| depth[a][k].partial_cmp(&depth[b][k]).unwrap()).unwrap();
        let mut want = plan_at(tau);
        if let Some(w) = cfg.winner { if tau > target - 10.0 * period { want = Some(w); } }
        for i in 0..n {
            ar[i] = 0.72 * ar[i] + (rng.next() - 0.5) * 0.16;
            let mut d = ar[i];
            if let Some(l) = want {
                if cur != l {
                    // close the gap to the leader within about five beats
                    let gap = depth[cur][k] - depth[l][k];
                    if i == l { d += ((gap + 30.0) / (h * 3.0)).clamp(0.3, 0.9); } else if i == cur { d -= 0.12; }
                } else if i == l { d += 0.04; }
            }
            depth[i][k + 1] = depth[i][k] + h * (1.0 + d.clamp(-0.3, 0.95));
        }
    }
    // ----- build a chain for every marble (named, then crowd)
    let mk_chain = |ys: &dyn Fn(usize) -> f64, lane: usize, jit: f64| -> Chain {
        let mut c = Chain::default();
        for k in 0..kk {
            let x = (lane_x(lane) + (swing[k] + jit).clamp(-wl, wl)).clamp(RAD + 6.0, W - RAD - 6.0);
            c.t.push(times[k]); c.x.push(x); c.y.push(ys(k));
        }
        arc(&mut c);
        c
    };
    let mut chains: Vec<Chain> = (0..n).map(|i| mk_chain(&|k| depth[i][k], named_lane[i], (rng.next() - 0.5) * 3.0)).collect();
    let leader_depth = |k: usize| (0..n).map(|i| depth[i][k]).fold(f64::MIN, f64::max);
    // crowd: present while the music is intense enough, sliding in from above and out again
    let mut crowd_active: Vec<Vec<f64>> = vec![]; // per crowd marble, per station: presence 0..1
    for j in 0..CROWD_MAX {
        let thr = 0.52 + 0.1 * j as f64;
        let mut pres = vec![0.0f64; kk];
        for k in 0..kk { let want = if inten(cfg.bars, times[k]) >= thr { 1.0 } else { 0.0 }; let prev = if k > 0 { pres[k - 1] } else { 0.0 }; pres[k] = prev + (want - prev).clamp(-0.34, 0.34); }
        crowd_active.push(pres);
    }
    let mut crowd_present = vec![false; CROWD_MAX];
    for j in 0..CROWD_MAX {
        let lane = free_lanes[j % free_lanes.len().max(1)];
        let off_j = 90.0 + rng.next() * 330.0;
        let pres = crowd_active[j].clone();
        let c = mk_chain(&|k| leader_depth(k) - off_j - (1.0 - pres[k]) * 1100.0, lane, (rng.next() - 0.5) * 3.0);
        crowd_present[j] = pres.iter().any(|p| *p > 0.0);
        chains.push(c);
    }
    let total = chains.len();
    // ----- finish line and per-frame sampling
    let last_named = (0..n).map(|i| depth[i][kk - 1]).fold(f64::MIN, f64::max);
    let finish_y = last_named - 4.0; // the leader crosses as it lands on the last beat
    let floor_y = finish_y + 170.0;
    let fps = FPS;
    let max_t = cfg.len * 1.6;
    let max_f = (max_t * fps) as usize + 2;
    let mut pos = vec![0f32; max_f * total * 2];
    let mut touch = vec![0u8; max_f * total];
    let mut lead = vec![0u8; max_f];
    let mut cam = vec![0f32; max_f];
    let mut events: Vec<Event> = vec![];
    let mut fins = vec![-1.0f64; total];
    let mut ptr = vec![0usize; total];
    let mut cam_y = 300.0f64;
    let (mut winner, mut win_f, mut frames) = (-1i64, -1i64, max_f);
    for f in 0..max_f {
        let t = f as f64 / fps;
        let mut named_y = vec![0f64; n];
        for m in 0..total {
            let c = &chains[m];
            let (x, y);
            if t <= c.t[0] { x = c.x[0]; y = if m < n { y0 } else { c.y[0] }; }
            else if t >= c.t[kk - 1] {
                let s = t - c.t[kk - 1];
                let (_, vy) = c.vout[kk - 1];
                // terminal velocity like the physics mode
                let tc = ((1300.0 - vy) / GRAV).max(0.0);
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
            // touching a pad within two frames of a station
            let near = c.t.iter().skip(1).any(|st| (st - t).abs() < 2.5 / fps);
            if near { touch[f * total + m] = 1; }
            if m < n && fins[m] < 0.0 && y > finish_y { fins[m] = t; if winner < 0 { winner = m as i64; win_f = f as i64; } }
        }
        let l = (0..n).max_by(|&a, &b| { let sa = if fins[a] >= 0.0 { 1e9 - fins[a] } else { named_y[a] }; let sb = if fins[b] >= 0.0 { 1e9 - fins[b] } else { named_y[b] }; sa.partial_cmp(&sb).unwrap() }).unwrap();
        lead[f] = l as u8;
        let ly = named_y.iter().cloned().fold(f64::MIN, f64::max);
        cam_y += (ly.min(finish_y - 80.0) - cam_y) * 0.1;
        cam[f] = cam_y as f32;
        if win_f >= 0 && f as i64 >= win_f + (3.0 * fps) as i64 { frames = f + 1; break; }
    }
    // hits: one event per station per marble that actually reaches the pad
    for (m, c) in chains.iter().enumerate() {
        if m >= n && !crowd_present[m - n] { continue; }
        for k in 1..kk - 1 {
            let dv = ((c.vout[k].0 - c.vin[k].0).hypot(c.vout[k].1 - c.vin[k].1)) as f32;
            if dv > 60.0 { events.push(Event { m: m as u8, f: (c.t[k] * fps).round() as u32, kind: 1, v: dv, x: c.x[k] as f32 }); }
        }
    }
    events.sort_by_key(|e| e.f);
    pos.truncate(frames * total * 2); touch.truncate(frames * total); lead.truncate(frames); cam.truncate(frames);
    let w = if winner >= 0 { winner as usize } else { lead[frames - 1] as usize };
    let twin = if fins[w] >= 0.0 { fins[w] } else { max_t };
    // the pads: a bumper where each marble touches, on the far side of the bounce, with its own life around the beat
    let mut segs: Vec<Seg> = vec![];
    for (m, c) in chains.iter().enumerate() {
        if m >= n && !crowd_present[m - n] { continue; }
        for k in 1..kk - 1 {
            let (dvx, dvy) = (c.vout[k].0 - c.vin[k].0, c.vout[k].1 - c.vin[k].1);
            let dl = dvx.hypot(dvy);
            if dl < 60.0 { continue; }
            let (nx, ny) = (dvx / dl, dvy / dl);
            let rb = if m < n { 15.0 } else { 11.0 };
            segs.push(Seg { x1: c.x[k] - nx * (RAD + rb), y1: c.y[k] - ny * (RAD + rb), x2: c.x[k] - nx * (RAD + rb), y2: c.y[k] - ny * (RAD + rb), r: rb, e: if m < n { 0.9 } else { 0.4 }, kind: Kind::Bump, t0: c.t[k], vis0: c.t[k] - 0.5, t1: c.t[k] + 1.4 });
        }
    }
    segs.push(Seg { x1: 0.0, y1: floor_y, x2: W, y2: floor_y, r: 6.0, e: 0.2, kind: Kind::Ramp, t0: -1e9, vis0: -1e9, t1: 1e9 });
    let map = &MAPS[MAPS.iter().position(|m| m.key == cfg.map).unwrap_or(0)];
    let course = Course { segs, spins: vec![], bands: vec![vec![]], finish_y, floor_y, hc: finish_y, pal: palette_for(map.key, cfg.seed), map_key: map.key };
    let sim = Sim { n: total, frames, pos, lead, cam, twin, winner: w, fins, events, touch };
    Some((course, sim, total))
}
