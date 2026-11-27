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

/// Spectral-flux onset envelope -> tempo by autocorrelation -> beat grid locked to the strongest onsets.
pub fn find_beats(sr: u32, mono: &[f32]) -> (Vec<f64>, f64) {
    let hop = (sr as usize / 100).max(1); // ~10 ms
    let win = 1024usize;
    let mut env = Vec::new();
    let mut prev = vec![0f32; win / 2];
    let hann: Vec<f32> = (0..win).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / win as f32).cos()).collect();
    // cheap DFT bands: 24 log-spaced bins via Goertzel
    let bins: Vec<f32> = (0..24).map(|k| 50.0 * (1.0 + k as f32 * 0.0) * 1.22f32.powi(k)).collect();
    let coefs: Vec<f32> = bins.iter().map(|f| 2.0 * (2.0 * std::f32::consts::PI * f / sr as f32).cos()).collect();
    let mut prevb = vec![0f32; bins.len()];
    let _ = &mut prev;
    let mut i = 0;
    while i + win <= mono.len() {
        let mut flux = 0.0;
        for (b, &c) in coefs.iter().enumerate() {
            let (mut s1, mut s2) = (0f32, 0f32);
            for n in 0..win { let s = mono[i + n] * hann[n] + c * s1 - s2; s2 = s1; s1 = s; }
            let mag = (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt();
            let m = (1.0 + mag).ln();
            flux += (m - prevb[b]).max(0.0);
            prevb[b] = m;
        }
        env.push(flux);
        i += hop;
    }
    if env.len() < 200 { return (vec![], 120.0); }
    // remove slow trend
    let w = 50;
    let eo: Vec<f32> = (0..env.len()).map(|i| { let a = i.saturating_sub(w); let b = (i + w).min(env.len() - 1); let m = env[a..=b].iter().sum::<f32>() / (b - a + 1) as f32; (env[i] - m).max(0.0) }).collect();
    let fps = sr as f64 / hop as f64;
    // tempo: autocorrelation over 70-180 BPM
    let (lo, hi) = ((fps * 60.0 / 180.0) as usize, (fps * 60.0 / 70.0) as usize);
    let mut best = (0usize, -1.0f32);
    for lag in lo..=hi.min(eo.len() / 2) {
        let mut s = 0.0;
        for i in 0..eo.len() - lag { s += eo[i] * eo[i + lag]; }
        // mild preference for ~120 BPM to avoid half/double errors
        let bpm = 60.0 * fps / lag as f64;
        let pref = (-((bpm / 120.0).ln()).powi(2) / 0.5).exp() as f32;
        s *= 0.6 + 0.4 * pref;
        if s > best.1 { best = (lag, s); }
    }
    let period = best.0 as f64;
    let bpm = 60.0 * fps / period;
    // phase: the offset whose comb picks up the most onset energy
    let mut bp = (0.0f64, -1.0f32);
    for ph in 0..best.0 {
        let mut s = 0.0;
        let mut k = ph as f64;
        while (k as usize) < eo.len() { s += eo[k as usize]; k += period; }
        if s > bp.1 { bp = (ph as f64, s); }
    }
    let dur = mono.len() as f64 / sr as f64;
    let mut beats = Vec::new();
    let mut t = bp.0 / fps;
    while t < dur { beats.push(t); t += period / fps; }
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
