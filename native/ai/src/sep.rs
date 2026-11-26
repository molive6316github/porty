//! Spleeter 2-stem vocal separation, native. Mirrors the web worker so results match: STFT 4096/1024, model estimates vocal magnitude,
//! soft mask, inverse STFT. instrumental = mix - vocals, so the two always add back to the original.
use ndarray::Array4;
use ort::{session::Session, value::Tensor};
use rustfft::{num_complex::Complex, FftPlanner};

const N: usize = 4096;
const H: usize = 1024;
const NB: usize = N / 2 + 1;
const FB: usize = 1024;
const T: usize = 512;

pub struct Separator { model: Session }

impl Separator {
    pub fn load(onnx: &str) -> Result<Self, String> {
        let mk = |e: ort::Error| e.to_string();
        Ok(Separator { model: Session::builder().map_err(mk)?.with_intra_threads(4).map_err(mk)?.commit_from_file(onnx).map_err(mk)? })
    }

    /// stereo at 44.1 kHz -> (vocals L, vocals R)
    pub fn vocals(&mut self, l: &[f32], r: &[f32]) -> Result<(Vec<f32>, Vec<f32>), String> {
        let mk = |e: ort::Error| e.to_string();
        let len = l.len();
        let frames = (len + H - 1) / H;
        let s = (frames + T - 1) / T;
        let win: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos()).collect();
        let mut planner = FftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(N);
        let inv = planner.plan_fft_inverse(N);
        let spec: Vec<Vec<Complex<f32>>> = [l, r].iter().map(|x| {
            let mut out = vec![Complex::new(0.0, 0.0); frames * NB];
            let mut buf = vec![Complex::new(0.0, 0.0); N];
            for t in 0..frames {
                let o = t * H;
                for i in 0..N { buf[i] = Complex::new(if o + i < x.len() { x[o + i] * win[i] } else { 0.0 }, 0.0); }
                fwd.process(&mut buf);
                out[t * NB..(t + 1) * NB].copy_from_slice(&buf[..NB]);
            }
            out
        }).collect();
        let mut x = Array4::<f32>::zeros((2, s, T, FB));
        for c in 0..2 { for t in 0..frames { for k in 0..FB { x[[c, t / T, t % T, k]] = spec[c][t * NB + k].norm(); } } }
        let shape = [2usize, s, T, FB];
        let inp = Tensor::from_array((shape, x.iter().cloned().collect::<Vec<f32>>())).map_err(mk)?;
        let out = self.model.run(ort::inputs!["x" => inp]).map_err(mk)?;
        let (_, yv) = out["y"].try_extract_tensor::<f32>().map_err(mk)?;
        let eps = 1e-10f32;
        let mut res: Vec<Vec<f32>> = vec![];
        for c in 0..2 {
            let mut ola = vec![0f32; frames * H + N];
            let mut wsum = vec![0f32; frames * H + N];
            let mut buf = vec![Complex::new(0.0, 0.0); N];
            for t in 0..frames {
                for k in 0..NB {
                    let m = if k < FB {
                        let v = yv[((c * s + t / T) * T + t % T) * FB + k];
                        let a = (x[[c, t / T, t % T, k]] - v).max(0.0);
                        (v * v + eps / 2.0) / (v * v + a * a + eps)
                    } else { 0.0 };
                    buf[k] = spec[c][t * NB + k] * m;
                }
                for k in 1..N / 2 { buf[N - k] = buf[k].conj(); }
                inv.process(&mut buf);
                let o = t * H;
                for i in 0..N { ola[o + i] += buf[i].re / N as f32 * win[i]; wsum[o + i] += win[i] * win[i]; }
            }
            res.push((0..len).map(|i| if wsum[i] > 1e-3 { ola[i] / wsum[i] } else { 0.0 }).collect());
        }
        let r2 = res.pop().unwrap();
        Ok((res.pop().unwrap(), r2))
    }
}
