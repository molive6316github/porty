//! kNN-VC voice conversion, native: WavLM-Large layer-6 features -> match each frame to the target voice's nearest frames -> HiFi-GAN.
use ndarray::{Array2, ArrayView2};
use ort::{session::Session, value::Tensor};

pub const D: usize = 1024;
const K: usize = 4;

pub struct Vc { wavlm: Session, hifi: Session }
pub struct Reference { raw: Array2<f32>, unit: Array2<f32> } // [M, D]

/// linear-interpolated resample (good enough for speech going into WavLM at 16 kHz)
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to { return x.to_vec(); }
    let ratio = from as f64 / to as f64;
    let n = (x.len() as f64 / ratio) as usize;
    // simple low-pass by box averaging when downsampling
    let src: Vec<f32> = if from > to { let w = (ratio.ceil() as usize).max(1); (0..x.len()).map(|i| { let a = i.saturating_sub(w / 2); let b = (i + w / 2 + 1).min(x.len()); x[a..b].iter().sum::<f32>() / (b - a) as f32 }).collect() } else { x.to_vec() };
    (0..n).map(|j| { let p = j as f64 * ratio; let i = p as usize; let f = (p - i as f64) as f32; let a = src[i.min(src.len() - 1)]; let b = src[(i + 1).min(src.len() - 1)]; a + (b - a) * f }).collect()
}

impl Vc {
    pub fn load(wavlm_parts: &[String], hifigan: &str) -> Result<Self, String> {
        let mut bytes = Vec::new();
        for p in wavlm_parts { bytes.extend(std::fs::read(p).map_err(|e| format!("{p}: {e}"))?); }
        let mk = |e: ort::Error| e.to_string();
        let wavlm = Session::builder().map_err(mk)?.with_intra_threads(4).map_err(mk)?.commit_from_memory(&bytes).map_err(mk)?;
        let hifi = Session::builder().map_err(mk)?.with_intra_threads(4).map_err(mk)?.commit_from_file(hifigan).map_err(mk)?;
        Ok(Vc { wavlm, hifi })
    }
    /// 16 kHz mono -> [T, 1024] features at 50 Hz
    pub fn features(&mut self, audio: &[f32]) -> Result<Array2<f32>, String> {
        let mk = |e: ort::Error| e.to_string();
        let mut rows: Vec<f32> = vec![];
        let mut t = 0usize;
        for seg in audio.chunks(160_000) {
            if seg.len() < 1600 { continue; }
            let mean = seg.iter().sum::<f32>() / seg.len() as f32;
            let var = seg.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / seg.len() as f32;
            let sd = (var + 1e-7).sqrt();
            let x: Vec<f32> = seg.iter().map(|v| (v - mean) / sd).collect();
            let inp = Tensor::from_array(([1usize, x.len()], x)).map_err(mk)?;
            let out = self.wavlm.run(ort::inputs!["input_values" => inp]).map_err(mk)?;
            let (shape, data) = out["hidden_states"].try_extract_tensor::<f32>().map_err(mk)?;
            t += shape[1] as usize;
            rows.extend_from_slice(data);
        }
        Array2::from_shape_vec((t, D), rows).map_err(|e| e.to_string())
    }
    pub fn reference(&mut self, audio_16k: &[f32]) -> Result<Reference, String> {
        let raw = self.features(audio_16k)?;
        let mut unit = raw.clone();
        for mut r in unit.rows_mut() { let n = r.iter().map(|x| x * x).sum::<f32>().sqrt() + 1e-8; r.mapv_inplace(|x| x / n); }
        Ok(Reference { raw, unit })
    }
    pub fn convert(&mut self, audio_16k: &[f32], r: &Reference) -> Result<Vec<f32>, String> {
        let src = self.features(audio_16k)?;
        let t = src.nrows();
        let mut su = src.clone();
        for mut row in su.rows_mut() { let n = row.iter().map(|x| x * x).sum::<f32>().sqrt() + 1e-8; row.mapv_inplace(|x| x / n); }
        let sims = su.dot(&r.unit.t()); // [T, M] cosine similarities
        let mut out = Array2::<f32>::zeros((t, D));
        for i in 0..t {
            let row = sims.row(i);
            let mut best: [(f32, usize); K] = [(-2.0, 0); K];
            for (m, &s) in row.iter().enumerate() {
                if s > best[K - 1].0 { let mut j = K - 1; while j > 0 && best[j - 1].0 < s { best[j] = best[j - 1]; j -= 1; } best[j] = (s, m); }
            }
            let mut o = out.row_mut(i);
            for (_, m) in best { o += &r.raw.row(m).mapv(|x| x / K as f32); }
        }
        self.vocode(out.view())
    }
    fn vocode(&mut self, f: ArrayView2<f32>) -> Result<Vec<f32>, String> {
        let mk = |e: ort::Error| e.to_string();
        let t = f.nrows();
        let mut x = vec![0f32; D * t];
        for i in 0..t { for d in 0..D { x[d * t + i] = f[[i, d]]; } }
        let inp = Tensor::from_array(([1usize, D, t], x)).map_err(mk)?;
        let out = self.hifi.run(ort::inputs!["features" => inp]).map_err(mk)?;
        let (_, w) = out["waveform"].try_extract_tensor::<f32>().map_err(mk)?;
        // the vocoder emits 256 samples per 20 ms feature frame (should be 320): stretch by 1.25, cubic interpolation
        let ratio = 320.0 / 256.0;
        let n = (w.len() as f64 * ratio) as usize;
        let g = |i: isize| w[i.clamp(0, w.len() as isize - 1) as usize];
        Ok((0..n).map(|j| {
            let p = j as f64 / ratio; let i = p.floor() as isize; let f = (p - i as f64) as f32;
            let (a, b, c, d) = (g(i - 1), g(i), g(i + 1), g(i + 2));
            b + 0.5 * f * (c - a + f * (2.0 * a - 5.0 * b + 4.0 * c - d + f * (3.0 * (b - c) + d - a)))
        }).collect())
    }
}
