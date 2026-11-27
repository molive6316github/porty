//! Decode any common audio file (mp3/wav/flac/ogg/aac/m4a) to mono f32, then find the beats.
use symphonia::core::{audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint};

pub struct Song {
    pub sr: u32,
    pub mono: Vec<f32>,
    pub stereo: Vec<f32>, // interleaved L,R for playback
    pub beats: Vec<f64>,
    pub bpm: f64,
}

pub fn decode(path: &str) -> Result<(u32, Vec<f32>, Vec<f32>), String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(f), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) { hint.with_extension(ext); }
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).map_err(|e| e.to_string())?;
    let mut fmt = probed.format;
    let tr = fmt.default_track().ok_or("no audio track")?.clone();
    let mut dec = symphonia::default::get_codecs().make(&tr.codec_params, &DecoderOptions::default()).map_err(|e| e.to_string())?;
    let sr = tr.codec_params.sample_rate.ok_or("unknown sample rate")?;
    let (mut mono, mut st) = (Vec::new(), Vec::new());
    loop {
        let pkt = match fmt.next_packet() { Ok(p) => p, Err(Error::IoError(_)) | Err(Error::ResetRequired) => break, Err(e) => return Err(e.to_string()) };
        if pkt.track_id() != tr.id { continue; }
        let buf = match dec.decode(&pkt) { Ok(b) => b, Err(Error::DecodeError(_)) => continue, Err(e) => return Err(e.to_string()) };
        let ch = buf.spec().channels.count();
        let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, *buf.spec());
        sb.copy_interleaved_ref(buf);
        for fr in sb.samples().chunks(ch) {
            let (l, r) = (fr[0], if ch > 1 { fr[1] } else { fr[0] });
            st.push(l);
            st.push(r);
            mono.push((l + r) * 0.5);
        }
    }
    if mono.is_empty() { return Err("no audio decoded".into()); }
    Ok((sr, mono, st))
}

/// Beat tracking. Spectral-flux onset envelope (5 ms resolution) -> coarse tempo -> fine tempo + phase search on a fractional grid
/// -> local drift correction every few beats, so the grid keeps following the music instead of slowly sliding off it.
pub fn find_beats(sr: u32, mono: &[f32]) -> (Vec<f64>, f64) {
    let hop = (sr as usize / 200).max(1); // 5 ms
    let win = 1024usize;
    let hann: Vec<f32> = (0..win).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / win as f32).cos()).collect();
    let bins: Vec<f32> = (0..24).map(|k| 50.0 * 1.22f32.powi(k)).collect();
    let coefs: Vec<f32> = bins.iter().map(|f| 2.0 * (2.0 * std::f32::consts::PI * f / sr as f32).cos()).collect();
    let mut prevb = vec![0f32; bins.len()];
    let mut env = Vec::new();
    let mut i = 0;
    while i + win <= mono.len() {
        let mut flux = 0.0;
        for (b, &c) in coefs.iter().enumerate() {
            let (mut s1, mut s2) = (0f32, 0f32);
            for n in 0..win { let s = mono[i + n] * hann[n] + c * s1 - s2; s2 = s1; s1 = s; }
            let m = (1.0 + (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt()).ln();
            flux += (m - prevb[b]).max(0.0);
            prevb[b] = m;
        }
        env.push(flux);
        i += hop;
    }
    if env.len() < 400 { return (vec![], 120.0); }
    let fps = sr as f64 / hop as f64;
    // an onset shows up in the flux when it is about a quarter of the way into the analysis window
    let lat = (win as f64 / 4.0) / sr as f64;
    let w = 100;
    let mut eo: Vec<f32> = (0..env.len()).map(|i| { let a = i.saturating_sub(w); let b = (i + w).min(env.len() - 1); let m = env[a..=b].iter().sum::<f32>() / (b - a + 1) as f32; (env[i] - m).max(0.0) }).collect();
    let sm: Vec<f32> = (0..eo.len()).map(|i| 0.25 * eo[i.saturating_sub(1)] + 0.5 * eo[i] + 0.25 * eo[(i + 1).min(eo.len() - 1)]).collect();
    eo = sm;
    // coarse tempo by autocorrelation, 70-180 BPM, gently preferring ~120
    let (lo, hi) = ((fps * 60.0 / 180.0) as usize, (fps * 60.0 / 70.0) as usize);
    let mut best = (lo, -1.0f32);
    for lag in lo..=hi.min(eo.len() / 2) {
        let mut sc = 0.0;
        for i in 0..eo.len() - lag { sc += eo[i] * eo[i + lag]; }
        let bpm = 60.0 * fps / lag as f64;
        sc *= 0.6 + 0.4 * (-((bpm / 120.0).ln()).powi(2) / 0.5).exp() as f32;
        if sc > best.1 { best = (lag, sc); }
    }
    let p0 = best.0 as f64;
    // dynamic-programming beat tracker (Ellis 2007): choose the chain of onsets that is strong and keeps close to the tempo,
    // but is free to speed up and slow down along the way
    let n = eo.len();
    let sd = (eo.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt().max(1e-9);
    let o: Vec<f32> = eo.iter().map(|v| v / sd).collect();
    let alpha = 140.0f32;
    let (lo_i, hi_i) = ((0.55 * p0) as usize, (1.8 * p0) as usize);
    let mut c = vec![0f32; n];
    let mut prev = vec![-1i64; n];
    for t in 0..n {
        let mut best = (0.0f32, -1i64);
        for d in lo_i.max(1)..=hi_i {
            if d > t { break; }
            let l = (d as f32 / p0 as f32).ln();
            let v = c[t - d] - alpha * l * l;
            if v > best.0 { best = (v, (t - d) as i64); }
        }
        c[t] = o[t] + best.0;
        prev[t] = best.1;
    }
    let tail = (n as f64 - 1.5 * p0).max(0.0) as usize;
    let mut t = (tail..n).max_by(|&a, &b| c[a].partial_cmp(&c[b]).unwrap()).unwrap() as i64;
    let mut frames: Vec<f64> = vec![];
    while t >= 0 {
        let i = t as usize;
        // sub-frame position by parabolic interpolation around the peak
        let (l, m, r) = (eo[i.saturating_sub(1)], eo[i], eo[(i + 1).min(n - 1)]);
        let den = l - 2.0 * m + r;
        let off = if den.abs() > 1e-9 { (0.5 * (l - r) / den).clamp(-0.5, 0.5) as f64 } else { 0.0 };
        frames.push(i as f64 + off);
        t = prev[i];
    }
    frames.reverse();
    if frames.len() < 8 { return (vec![], 60.0 * fps / p0); }
    // flux peaks sit a fixed amount after the true onset; remove that (calibrated against known click tracks)
    let delay = 0.0205;
    // drop stray weak beats at the very start / end (the chain is free to begin and end anywhere)
    let strength: Vec<f32> = frames.iter().map(|f| o[(*f as usize).min(n - 1)]).collect();
    let mut ss = strength.clone(); ss.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = ss[ss.len() / 2];
    let (mut a0, mut z0) = (0usize, frames.len());
    while a0 + 8 < z0 && strength[a0] < 0.3 * med { a0 += 1; }
    while z0 > a0 + 8 && strength[z0 - 1] < 0.3 * med { z0 -= 1; }
    let frames: Vec<f64> = frames[a0..z0].to_vec();
    let beats: Vec<f64> = frames.iter().map(|f| f / fps + delay - lat).filter(|t| *t >= 0.0).collect();
    let mut iv: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
    iv.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let bpm = 60.0 / iv[iv.len() / 2];
    (beats, bpm)
}

pub fn load_song(path: &str) -> Result<Song, String> {
    let (sr, mono, stereo) = decode(path)?;
    let (beats, bpm) = find_beats(sr, &mono);
    Ok(Song { sr, mono, stereo, beats, bpm })
}

/// Bars (4 beats) of the song: returns (downbeat times, energy per bar). The downbeat phase is the one whose beats are loudest.
pub fn bars(song: &Song) -> (Vec<f64>, Vec<f32>) {
    let b = &song.beats;
    if b.len() < 8 { return (vec![], vec![]); }
    let hop = (song.sr as usize / 100).max(1);
    let env: Vec<f32> = song.mono.chunks(hop).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()).collect();
    let at = |t: f64| { let i = (t * 100.0) as usize; env[i.min(env.len() - 1)..(i + 6).min(env.len())].iter().cloned().fold(0.0f32, f32::max) };
    let mut best = (0usize, -1.0f32);
    for ph in 0..4 { let s: f32 = (ph..b.len()).step_by(4).map(|k| at(b[k])).sum(); if s > best.1 { best = (ph, s); } }
    let downs: Vec<f64> = (best.0..b.len()).step_by(4).map(|k| b[k]).collect();
    let energy: Vec<f32> = downs.iter().enumerate().map(|(i, &t0)| { let t1 = downs.get(i + 1).copied().unwrap_or(t0 + 2.0); let (a, z) = ((t0 * 100.0) as usize, ((t1 * 100.0) as usize).min(env.len())); if z > a { env[a..z].iter().sum::<f32>() / (z - a) as f32 } else { 0.0 } }).collect();
    (downs, energy)
}

/// The song's big moments (drops, section changes), snapped to bar starts, at least `min_gap` seconds apart. Strongest energy jumps first.
pub fn section_times(song: &Song, count: usize, min_gap: f64) -> Vec<f64> {
    let (downs, e) = bars(song);
    if downs.len() < 4 { return vec![]; }
    // novelty: how much louder this bar is than the average of the previous two (and the reverse, for sudden drops)
    let mut cand: Vec<(f32, f64)> = vec![];
    for i in 2..downs.len() {
        let prev = (e[i - 1] + e[i - 2]) * 0.5 + 1e-5;
        cand.push(((e[i] / prev - 1.0).abs(), downs[i]));
    }
    cand.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let dur = song.mono.len() as f64 / song.sr as f64;
    let mut out: Vec<f64> = vec![];
    for (_, t) in cand { if t > 4.0 && t < dur - 8.0 && out.iter().all(|x| (x - t).abs() >= min_gap) { out.push(t); if out.len() >= count { break; } } }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A click track whose tempo drifts 122 -> 132 BPM must be followed to within a few milliseconds.
    #[test]
    fn follows_tempo_changes() {
        let sr = 22050u32;
        let dur = 40.0;
        let mut x = vec![0f32; (sr as f64 * dur) as usize];
        let mut truth = vec![];
        let mut t = 0.6;
        let mut i = 0;
        while t < dur - 0.3 {
            truth.push(t);
            let k = (t * sr as f64) as usize;
            for m in 0..1500 { let ph = 2.0 * std::f32::consts::PI * (80.0 + 60.0 * (-(m as f32) / 150.0).exp()) * m as f32 / sr as f32; if k + m < x.len() { x[k + m] += ph.sin() * (-(m as f32) / 350.0).exp() * if i % 4 == 0 { 1.0 } else { 0.7 }; } }
            t += 60.0 / (122.0 + 10.0 * t / dur);
            i += 1;
        }
        let (beats, _bpm) = find_beats(sr, &x);
        assert!((beats.len() as i64 - truth.len() as i64).abs() <= 1, "{} vs {}", beats.len(), truth.len());
        let worst = beats.iter().map(|b| truth.iter().map(|t| (b - t).abs()).fold(f64::MAX, f64::min)).fold(0.0, f64::max);
        assert!(worst < 0.012, "worst beat error {worst}");
    }
}
