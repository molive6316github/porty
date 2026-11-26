//! Lyrics sync + singing: align the user's lyrics to what Whisper heard, then have a voice sing each line in its time slot.
use crate::{piper::Piper, vc::resample, whisper::Word};

#[derive(Clone, Debug)]
pub struct Line { pub text: String, pub t0: f64, pub t1: f64, pub words: Vec<(String, f64)> }

fn norm(w: &str) -> String { w.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect() }
fn sim(a: &str, b: &str) -> f32 {
    if a.is_empty() || b.is_empty() { return -1.0; }
    if a == b { return 2.0; }
    // one edit away counts as a near match (whisper mishears small words)
    let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if x.len().abs_diff(y.len()) > 1 { return -1.0; }
    let mut dp: Vec<usize> = (0..=y.len()).collect();
    for i in 1..=x.len() { let mut prev = dp[0]; dp[0] = i; for j in 1..=y.len() { let t = dp[j]; dp[j] = (dp[j] + 1).min(dp[j - 1] + 1).min(prev + (x[i - 1] != y[j - 1]) as usize); prev = t; } }
    if dp[y.len()] <= 1 && x.len().min(y.len()) >= 3 { 1.0 } else { -1.0 }
}

/// Semi-global Needleman-Wunsch: every lyric word is placed, the heard words may have free extras at either end.
/// Returns a start time per lyric word (matched words use the heard time; the rest are interpolated).
pub fn align(lyrics: &str, heard: &[Word], total: f64) -> Vec<Line> {
    let lines: Vec<Vec<String>> = lyrics.lines().map(|l| l.split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>()).filter(|l| !l.is_empty()).collect();
    let flat: Vec<(usize, String)> = lines.iter().enumerate().flat_map(|(i, l)| l.iter().map(move |w| (i, w.clone()))).collect();
    let (n, m) = (flat.len(), heard.len());
    if n == 0 { return vec![]; }
    let a: Vec<String> = flat.iter().map(|(_, w)| norm(w)).collect();
    let b: Vec<String> = heard.iter().map(|w| norm(&w.text)).collect();
    let gap = -1.0f32;
    let mut d = vec![vec![0f32; m + 1]; n + 1];
    let mut tb = vec![vec![0u8; m + 1]; n + 1];
    for i in 1..=n { d[i][0] = d[i - 1][0] + gap; tb[i][0] = 1; }
    for i in 1..=n { for j in 1..=m {
        let diag = d[i - 1][j - 1] + sim(&a[i - 1], &b[j - 1]);
        let up = d[i - 1][j] + gap;    // lyric word with no heard word
        let left = d[i][j - 1] + if i == n { 0.0 } else { gap * 0.5 }; // a heard word with no lyric
        let (v, t) = if diag >= up && diag >= left { (diag, 0) } else if up >= left { (up, 1) } else { (left, 2) };
        d[i][j] = v; tb[i][j] = t;
    } }
    // free trailing heard words: start traceback at the best j on the last row
    let mut j = (0..=m).max_by(|&x, &y| d[n][x].partial_cmp(&d[n][y]).unwrap()).unwrap();
    let mut i = n;
    let mut t_of: Vec<Option<f64>> = vec![None; n];
    while i > 0 {
        if j == 0 { i -= 1; continue; }
        match tb[i][j] {
            0 => { if sim(&a[i - 1], &b[j - 1]) > 0.0 { t_of[i - 1] = Some(heard[j - 1].start); } i -= 1; j -= 1; }
            1 => i -= 1,
            _ => j -= 1,
        }
    }
    // interpolate the unmatched, extrapolating at the ends by ~0.3 s a word
    let known: Vec<usize> = (0..n).filter(|&k| t_of[k].is_some()).collect();
    let mut t = vec![0f64; n];
    if known.is_empty() {
        let span = (total - 1.0).max(1.0);
        for k in 0..n { t[k] = 0.5 + span * k as f64 / n as f64; }
    } else {
        for k in 0..n {
            t[k] = if let Some(v) = t_of[k] { v } else {
                let prev = known.iter().rev().find(|&&x| x < k).copied();
                let next = known.iter().find(|&&x| x > k).copied();
                match (prev, next) {
                    (Some(p), Some(q)) => { let (tp, tq) = (t_of[p].unwrap(), t_of[q].unwrap()); tp + (tq - tp) * (k - p) as f64 / (q - p) as f64 }
                    (Some(p), None) => t_of[p].unwrap() + 0.3 * (k - p) as f64,
                    (None, Some(q)) => (t_of[q].unwrap() - 0.3 * (q - k) as f64).max(0.0),
                    _ => 0.0,
                }
            };
        }
        for k in 1..n { if t[k] < t[k - 1] { t[k] = t[k - 1] + 0.05; } }
    }
    let mut out: Vec<Line> = vec![];
    let mut k = 0;
    for (li, l) in lines.iter().enumerate() {
        let ws: Vec<(String, f64)> = l.iter().map(|w| { let x = (w.clone(), t[k]); k += 1; x }).collect();
        let t0 = ws[0].1;
        out.push(Line { text: l.join(" "), t0, t1: t0, words: ws });
        let _ = li;
    }
    for i in 0..out.len() {
        let last = out[i].words.last().unwrap().1;
        let next = out.get(i + 1).map(|l| l.t0).unwrap_or(total);
        out[i].t1 = (last + 0.6).min(next - 0.05).max(out[i].t0 + 0.3).min(total);
    }
    out
}

/// Sing every line with the voice picked by `speaker_at(time)`. Returns mono audio at `sr`, `total` seconds long.
pub fn sing(piper: &mut Piper, lines: &[Line], total: f64, sr: u32, speaker_at: &dyn Fn(f64) -> i64, mut progress: impl FnMut(usize, usize)) -> Result<Vec<f32>, String> {
    let mut out = vec![0f32; (total * sr as f64) as usize + sr as usize];
    for (i, l) in lines.iter().enumerate() {
        progress(i, lines.len());
        let ids = piper.phoneme_ids(&l.text)?;
        if ids.len() < 4 { continue; }
        let spk = speaker_at(l.t0);
        let want = (l.t1 - l.t0).max(0.3);
        let first = piper.synth(&ids, spk, Some(1.0), None, None)?;
        let natural = first.len() as f64 / piper.sample_rate as f64;
        // squeeze or stretch to the time slot with Piper's own duration control (more natural than resampling)
        let scale = (want / natural).clamp(0.55, 2.2) as f32;
        let audio = if (scale - 1.0).abs() < 0.05 { first } else { piper.synth(&ids, spk, Some(scale), None, None)? };
        let x = resample(&audio, piper.sample_rate, sr);
        let start = (l.t0 * sr as f64) as usize;
        let fade = (0.01 * sr as f64) as usize;
        for (k, v) in x.iter().enumerate() {
            let idx = start + k;
            if idx >= out.len() { break; }
            let g = (k.min(x.len() - 1 - k) as f32 / fade as f32).min(1.0);
            out[idx] += v * g;
        }
    }
    Ok(out)
}

/// Snap each line's start to the first real vocal onset near Whisper's estimate, using the separated vocal stem (16 kHz).
/// Whisper word starts run a little late and the first line tends to swallow the leading silence.
pub fn refine_starts(lines: &mut [Line], vocal16k: &[f32]) {
    let hop = 160usize; // 10 ms
    let env: Vec<f32> = vocal16k.chunks(hop).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt()).collect();
    for i in 0..lines.len() {
        let t0 = lines[i].t0;
        let (a, b) = (((t0 - 0.8).max(0.0) * 100.0) as usize, (((t0 + 1.6) * 100.0) as usize).min(env.len()));
        if b <= a + 10 { continue; }
        let mx = env[a..b].iter().cloned().fold(0.0f32, f32::max);
        if mx < 0.004 { continue; }
        let thr = mx * 0.04;
        // earliest frame whose energy stays above the threshold for 120 ms, preceded by quiet
        let mut found = None;
        for k in a..b.saturating_sub(8) {
            if env[k..k + 8].iter().all(|v| *v > thr) && (k < 3 || env[k - 3..k].iter().any(|v| *v < thr)) { found = Some(k); break; }
        }
        if let Some(mut k) = found {
            // walk back down the rise to its foot (soft consonants start well below the loud vowel)
            let mut steps = 0;
            while k > a + 1 && steps < 30 && env[k - 1] > mx * 0.006 && env[k - 1] <= env[k] * 1.15 { k -= 1; steps += 1; }
            let nt = k as f64 / 100.0;
            let shift = nt - lines[i].t0;
            let prev_end = if i > 0 { lines[i - 1].t0 + 0.2 } else { 0.0 };
            if nt > prev_end { lines[i].t0 = nt; lines[i].t1 = (lines[i].t1 + shift).max(nt + 0.3); for w in &mut lines[i].words { w.1 += shift; } }
        }
    }
}
