//! Whisper-tiny.en, native: log-mel -> encoder -> greedy decoder with KV cache -> word timestamps from cross-attention DTW.
use ndarray::{Array2, Array3};
use ort::{session::Session, value::Tensor};
use rustfft::{num_complex::Complex, FftPlanner};
use std::collections::HashMap;

const SR: usize = 16000;
const NFFT: usize = 400;
const HOP: usize = 160;
const NMEL: usize = 80;
const SOT: i64 = 50257;
const EOT: i64 = 50256;
const NOTS: i64 = 50362;
const LAYERS: usize = 4;
const HEADS: [(usize, usize); 8] = [(1, 0), (2, 0), (2, 5), (3, 0), (3, 1), (3, 2), (3, 3), (3, 4)];

#[derive(Clone, Debug)]
pub struct Word { pub text: String, pub start: f64, pub end: f64 }

pub struct Whisper { enc: Session, dec: Session, bytes: Vec<Vec<u8>>, suppress: Vec<usize>, filters: Array2<f32> }

fn hz_to_mel(f: f64) -> f64 { if f < 1000.0 { f / (200.0 / 3.0) } else { 15.0 + (f / 1000.0).ln() / (6.4f64.ln() / 27.0) } }
fn mel_to_hz(m: f64) -> f64 { if m < 15.0 { m * 200.0 / 3.0 } else { 1000.0 * ((6.4f64.ln() / 27.0) * (m - 15.0)).exp() } }

fn mel_filters() -> Array2<f32> {
    let nb = NFFT / 2 + 1;
    let (lo, hi) = (hz_to_mel(0.0), hz_to_mel(8000.0));
    let pts: Vec<f64> = (0..NMEL + 2).map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (NMEL + 1) as f64)).collect();
    let mut f = Array2::<f32>::zeros((NMEL, nb));
    for m in 0..NMEL {
        let (a, b, c) = (pts[m], pts[m + 1], pts[m + 2]);
        for k in 0..nb {
            let hz = k as f64 * SR as f64 / NFFT as f64;
            let w = ((hz - a) / (b - a)).min((c - hz) / (c - b)).max(0.0);
            f[[m, k]] = (w * 2.0 / (c - a)) as f32; // slaney area normalisation
        }
    }
    f
}

fn log_mel(audio: &[f32], filters: &Array2<f32>) -> Array2<f32> {
    // pad / trim to 30 s, reflect-pad for centred STFT
    let mut x = audio.to_vec();
    x.resize(30 * SR, 0.0);
    let p = NFFT / 2;
    let mut padded = Vec::with_capacity(x.len() + 2 * p);
    for i in (1..=p).rev() { padded.push(x[i]); }
    padded.extend_from_slice(&x);
    for i in 0..p { padded.push(x[x.len() - 2 - i]); }
    let frames = 3000;
    let win: Vec<f32> = (0..NFFT).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / NFFT as f32).cos()).collect();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(NFFT);
    let nb = NFFT / 2 + 1;
    let mut power = Array2::<f32>::zeros((nb, frames));
    let mut buf = vec![Complex::new(0.0, 0.0); NFFT];
    for t in 0..frames {
        for i in 0..NFFT { buf[i] = Complex::new(padded[t * HOP + i] * win[i], 0.0); }
        fft.process(&mut buf);
        for k in 0..nb { power[[k, t]] = buf[k].norm_sqr(); }
    }
    let mel = filters.dot(&power);
    let mut lm = mel.mapv(|v| v.max(1e-10).log10());
    let mx = lm.iter().cloned().fold(f32::MIN, f32::max);
    lm.mapv_inplace(|v| (v.max(mx - 8.0) + 4.0) / 4.0);
    lm
}

fn byte_decoder() -> HashMap<char, u8> {
    let mut bs: Vec<u32> = (b'!' as u32..=b'~' as u32).chain(0xA1..=0xAC).chain(0xAE..=0xFF).collect();
    let mut cs = bs.clone();
    let mut n = 0;
    for b in 0..256u32 { if !bs.contains(&b) { bs.push(b); cs.push(256 + n); n += 1; } }
    bs.iter().zip(cs.iter()).map(|(b, c)| (char::from_u32(*c).unwrap(), *b as u8)).collect()
}

impl Whisper {
    pub fn load(dir: &str) -> Result<Self, String> {
        let mk = |e: ort::Error| e.to_string();
        let enc = Session::builder().map_err(mk)?.with_intra_threads(4).map_err(mk)?.commit_from_file(format!("{dir}/onnx/encoder_model_quantized.onnx")).map_err(mk)?;
        let dec = Session::builder().map_err(mk)?.with_intra_threads(4).map_err(mk)?.commit_from_file(format!("{dir}/onnx/decoder_model_merged_quantized.onnx")).map_err(mk)?;
        let tj: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/tokenizer.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let bd = byte_decoder();
        let mut bytes = vec![vec![]; 51865];
        for (tok, id) in tj["model"]["vocab"].as_object().ok_or("no vocab")? {
            let id = id.as_u64().unwrap() as usize;
            if id < bytes.len() { bytes[id] = tok.chars().filter_map(|c| bd.get(&c).copied()).collect(); }
        }
        let gc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/generation_config.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let suppress = gc["suppress_tokens"].as_array().map(|a| a.iter().filter_map(|x| x.as_u64().map(|v| v as usize)).collect()).unwrap_or_default();
        Ok(Whisper { enc, dec, bytes, suppress, filters: mel_filters() })
    }

    /// 16 kHz mono (any length): returns words with start/end seconds. Processes 30 s windows.
    pub fn transcribe(&mut self, audio: &[f32]) -> Result<Vec<Word>, String> {
        let mut words = vec![];
        let win = 30 * SR;
        let mut off = 0;
        while off < audio.len() {
            let seg = &audio[off..(off + win).min(audio.len())];
            if seg.len() > SR / 2 {
                let mut w = self.chunk(seg)?;
                for x in &mut w { x.start += off as f64 / SR as f64; x.end += off as f64 / SR as f64; }
                words.extend(w);
            }
            off += win;
        }
        Ok(words)
    }

    fn chunk(&mut self, seg: &[f32]) -> Result<Vec<Word>, String> {
        let mk = |e: ort::Error| e.to_string();
        let mel = log_mel(seg, &self.filters);
        let feats = Tensor::from_array(([1usize, NMEL, 3000], mel.iter().cloned().collect::<Vec<f32>>())).map_err(mk)?;
        let eo = self.enc.run(ort::inputs!["input_features" => feats]).map_err(mk)?;
        let (_, hs) = eo["last_hidden_state"].try_extract_tensor::<f32>().map_err(mk)?;
        let hs: Vec<f32> = hs.to_vec();
        drop(eo);

        let mut tokens: Vec<i64> = vec![SOT, NOTS];
        let mut past: Vec<(Vec<usize>, Vec<f32>)> = vec![]; // 16 kv tensors (shape, data)
        let mut rows: Vec<Vec<f32>> = vec![]; // per input token: [head][1500] flattened
        let mut input: Vec<i64> = tokens.clone();
        let mut out_ids: Vec<i64> = vec![];
        for step in 0..200 {
            let cache = step > 0;
            let n = input.len();
            let ids = Tensor::from_array(([1usize, n], input.clone())).map_err(mk)?;
            let ehs = Tensor::from_array(([1usize, 1500, 384], hs.clone())).map_err(mk)?;
            let mut feed: Vec<(String, ort::session::SessionInputValue<'_>)> = vec![("input_ids".into(), ids.into()), ("encoder_hidden_states".into(), ehs.into())];
            for l in 0..LAYERS { for (who, kv) in [("decoder", "key"), ("decoder", "value"), ("encoder", "key"), ("encoder", "value")] {
                let idx = l * 4 + match (who, kv) { ("decoder", "key") => 0, ("decoder", "value") => 1, ("encoder", "key") => 2, _ => 3 };
                let (shape, data) = if cache { past[idx].clone() } else { (vec![1, 6, 0, 64], vec![]) };
                let t: ort::session::SessionInputValue<'_> = if cache { Tensor::from_array((shape, data)).map_err(mk)?.into() } else { Tensor::<f32>::new(&ort::memory::Allocator::default(), [1usize, 6, 0, 64]).map_err(mk)?.into() };
                feed.push((format!("past_key_values.{l}.{who}.{kv}"), t));
            } }
            feed.push(("use_cache_branch".into(), Tensor::from_array(([1usize], vec![cache])).map_err(mk)?.into()));
            let out = self.dec.run(feed).map_err(mk)?;
            let (ls, logits) = out["logits"].try_extract_tensor::<f32>().map_err(mk)?;
            let vocab = ls[2] as usize;
            let last = &logits[(ls[1] as usize - 1) * vocab..];
            // greedy with suppression (timestamps and the usual junk are never allowed; first token can't be a space/eot)
            let mut best = (f32::MIN, 0usize);
            for (i, &v) in last.iter().enumerate().take(50257) {
                if self.suppress.contains(&i) { continue; }
                if step == 0 && (i == 220 || i == EOT as usize) { continue; }
                if v > best.0 { best = (v, i); }
            }
            let next = if last[EOT as usize] > best.0 { EOT } else { best.1 as i64 };
            // cross-attention rows of the alignment heads for each input token of this step
            for q in 0..n {
                let mut r = Vec::with_capacity(HEADS.len() * 1500);
                for (l, h) in HEADS {
                    let (cs, ca) = out[format!("cross_attentions.{l}")].try_extract_tensor::<f32>().map_err(mk)?;
                    let qn = cs[2] as usize;
                    let base = (h * qn + q) * 1500;
                    r.extend_from_slice(&ca[base..base + 1500]);
                }
                rows.push(r);
            }
            // new cache: decoder kv grows each step, encoder kv is fixed after step 0
            let mut np = Vec::with_capacity(LAYERS * 4);
            for l in 0..LAYERS { for (who, kv) in [("decoder", "key"), ("decoder", "value"), ("encoder", "key"), ("encoder", "value")] {
                if cache && who == "encoder" { np.push(past[np.len()].clone()); }
                else { let (s, d) = out[format!("present.{l}.{who}.{kv}")].try_extract_tensor::<f32>().map_err(mk)?; np.push((s.iter().map(|x| *x as usize).collect(), d.to_vec())); }
            } }
            past = np;
            if next == EOT { break; }
            out_ids.push(next);
            tokens.push(next);
            input = vec![next];
        }
        if out_ids.is_empty() { return Ok(vec![]); }
        Ok(self.align(&out_ids, &rows, seg.len()))
    }

    fn align(&self, text: &[i64], rows: &[Vec<f32>], nsamples: usize) -> Vec<Word> {
        let ntok = text.len() + 1; // rows: [notimestamps, t1..tn] (drop the sot row)
        let frames = ((nsamples / HOP) / 2).clamp(1, 1500);
        let nh = HEADS.len();
        // [heads][ntok][frames], standardised per head over frames, median-filtered over time, averaged over heads
        let mut m = Array2::<f32>::zeros((ntok, frames));
        for h in 0..nh {
            let mut a = Array2::<f32>::zeros((ntok, frames));
            for t in 0..ntok { for f in 0..frames { a[[t, f]] = rows[t + 1][h * 1500 + f]; } }
            // standardise over the frame axis of each token row group (openai: over the time axis, per head across all rows)
            let n = (ntok * frames) as f32;
            let mean = a.iter().sum::<f32>() / n;
            let sd = (a.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n).sqrt() + 1e-9;
            let _ = (mean, sd);
            for t in 0..ntok {
                let row = a.row(t);
                let mu = row.iter().sum::<f32>() / frames as f32;
                let s = (row.iter().map(|v| (v - mu) * (v - mu)).sum::<f32>() / frames as f32).sqrt() + 1e-9;
                let z: Vec<f32> = row.iter().map(|v| (v - mu) / s).collect();
                // median filter width 7
                for f in 0..frames {
                    let mut w: Vec<f32> = (0..7).map(|k| { let i = (f as isize + k as isize - 3).clamp(0, frames as isize - 1) as usize; z[i] }).collect();
                    w.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    m[[t, f]] += w[3] / nh as f32;
                }
            }
        }
        // DTW over -m
        let cost = m.mapv(|v| -v);
        let mut d = Array2::<f32>::from_elem((ntok + 1, frames + 1), f32::INFINITY);
        let mut tr = Array2::<u8>::zeros((ntok + 1, frames + 1));
        d[[0, 0]] = 0.0;
        for i in 1..=ntok { for j in 1..=frames {
            let (c0, c1, c2) = (d[[i - 1, j - 1]], d[[i - 1, j]], d[[i, j - 1]]);
            let (c, t) = if c0 < c1 && c0 < c2 { (c0, 0) } else if c1 < c0 && c1 < c2 { (c1, 1) } else { (c2, 2) };
            d[[i, j]] = cost[[i - 1, j - 1]] + c;
            tr[[i, j]] = t;
        } }
        let (mut i, mut j) = (ntok, frames);
        let mut path: Vec<(usize, usize)> = vec![];
        while i > 0 || j > 0 {
            path.push((i.saturating_sub(1), j.saturating_sub(1)));
            match if i == 0 { 2 } else if j == 0 { 1 } else { tr[[i, j]] } { 0 => { i -= 1; j -= 1; } 1 => i -= 1, _ => j -= 1 }
        }
        path.reverse();
        // token start time = first frame where its row begins on the path
        let mut jump = vec![0f64; ntok];
        let mut seen = vec![false; ntok];
        for (ti, fj) in &path { if !seen[*ti] { seen[*ti] = true; jump[*ti] = *fj as f64 * 0.02; } }
        // words: a token starting with a space begins a new word. row k+1 belongs to text token k
        let mut words: Vec<(String, f64)> = vec![];
        for (k, &id) in text.iter().enumerate() {
            let b = &self.bytes[id as usize];
            let s = String::from_utf8_lossy(b).to_string();
            let start = jump[k];
            if s.starts_with(' ') || words.is_empty() { words.push((s.trim_start().to_string(), start)); }
            else { words.last_mut().unwrap().0.push_str(&s); }
        }
        let end_all = (nsamples as f64 / SR as f64).min(frames as f64 * 0.04);
        let n = words.len();
        (0..n).map(|k| Word { text: words[k].0.clone(), start: words[k].1, end: if k + 1 < n { words[k + 1].1 } else { end_all.max(words[k].1 + 0.2) } }).filter(|w| !w.text.is_empty()).collect()
    }
}
