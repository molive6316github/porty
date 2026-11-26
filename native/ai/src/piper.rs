//! Piper VITS text-to-speech, native: espeak-ng (pure Rust) -> IPA -> phoneme ids -> ONNX Runtime -> audio.
use ort::{session::Session, value::Tensor};
use std::collections::HashMap;

pub struct Piper {
    session: Session,
    ids: HashMap<String, Vec<i64>>,
    maxkey: usize,
    pub sample_rate: u32,
    pub speakers: usize,
    pub noise: f32,
    pub length: f32,
    pub noise_w: f32,
    voice: String,
    engine: espeak_ng::EspeakNg,
}

impl Piper {
    pub fn load(onnx: &str, json: &str) -> Result<Self, String> {
        let cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(json).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut ids = HashMap::new();
        for (k, v) in cfg["phoneme_id_map"].as_object().ok_or("no phoneme_id_map")? {
            ids.insert(k.clone(), v.as_array().unwrap().iter().filter_map(|x| x.as_i64()).collect::<Vec<i64>>());
        }
        let session = Session::builder().map_err(|e| e.to_string())?.with_intra_threads(4).map_err(|e| e.to_string())?.commit_from_file(onnx).map_err(|e| e.to_string())?;
        let voice = cfg["espeak"]["voice"].as_str().unwrap_or("en-gb").to_string();
        let base = std::env::var("LOCALAPPDATA").map(std::path::PathBuf::from).or_else(|_| std::env::var("HOME").map(|h| std::path::Path::new(&h).join(".cache"))).unwrap_or_else(|_| std::env::temp_dir());
        let dd = base.join("Marbeat").join("espeak-data");
        if !dd.join("phontab").exists() { std::fs::create_dir_all(&dd).map_err(|e| e.to_string())?; espeak_ng::install_bundled_languages(&dd, espeak_ng::bundled_languages()).map_err(|e| e.to_string())?; }
        let engine = espeak_ng::EspeakNg::with_data_dir(&voice, &dd).map_err(|e| format!("{e:?}"))?;
        let inf = &cfg["inference"];
        Ok(Piper {
            session, maxkey: ids.keys().map(|k| k.chars().count()).max().unwrap_or(1), ids,
            sample_rate: cfg["audio"]["sample_rate"].as_u64().unwrap_or(22050) as u32,
            speakers: cfg["speaker_id_map"].as_object().map(|m| m.len()).unwrap_or(1),
            noise: inf["noise_scale"].as_f64().unwrap_or(0.667) as f32,
            length: inf["length_scale"].as_f64().unwrap_or(1.0) as f32,
            noise_w: inf["noise_w"].as_f64().unwrap_or(0.8) as f32,
            voice, engine,
        })
    }
    pub fn ipa(&self, text: &str) -> Result<String, String> { self.engine.text_to_phonemes(text).map_err(|e| format!("{e:?}")) }
    fn push_ipa(&self, ipa: &str, out: &mut Vec<i64>) {
        let cs: Vec<char> = ipa.chars().filter(|c| *c != '\n').collect();
        let mut i = 0;
        while i < cs.len() {
            let mut hit = false;
            for l in (1..=1usize.min(cs.len() - i)).rev() {
                let k: String = cs[i..i + l].iter().collect();
                if let Some(v) = self.ids.get(&k) { for x in v { out.push(*x); out.push(0); } i += l; hit = true; break; }
            }
            if !hit { i += 1; }
        }
    }
    /// text -> espeak IPA per clause -> Piper phoneme ids: BOS, pad, then (phoneme, pad) pairs, punctuation kept, EOS
    pub fn phoneme_ids(&self, text: &str) -> Result<Vec<i64>, String> {
        let mut out = vec![1i64, 0];
        let mut seg = String::new();
        let chars: Vec<char> = text.chars().collect();
        let flush = |seg: &mut String, punct: Option<char>, last: bool, out: &mut Vec<i64>| -> Result<(), String> {
            let t = seg.trim();
            if !t.is_empty() { let ipa = self.engine.text_to_phonemes(t).map_err(|e| format!("{e:?}"))?; self.push_ipa(&ipa, out); }
            if let Some(p) = punct { if let Some(v) = self.ids.get(&p.to_string()) { for x in v { out.push(*x); out.push(0); } } }
            if !last && (punct.is_some() || !t.is_empty()) { if let Some(v) = self.ids.get(" ") { for x in v { out.push(*x); out.push(0); } } }
            seg.clear();
            Ok(())
        };
        for (i, c) in chars.iter().enumerate() {
            if ",.;:!?".contains(*c) { let last = chars[i + 1..].iter().all(|x| x.is_whitespace()); flush(&mut seg, Some(*c), last, &mut out)?; } else { seg.push(*c); }
        }
        flush(&mut seg, None, true, &mut out)?;
        out.push(2);
        Ok(out)
    }
    pub fn synth(&mut self, ids: &[i64], speaker: i64, length: Option<f32>, noise: Option<f32>, noise_w: Option<f32>) -> Result<Vec<f32>, String> {
        let n = ids.len();
        let input = Tensor::from_array(([1usize, n], ids.to_vec())).map_err(|e| e.to_string())?;
        let lens = Tensor::from_array(([1usize], vec![n as i64])).map_err(|e| e.to_string())?;
        let scales = Tensor::from_array(([3usize], vec![noise.unwrap_or(self.noise), length.unwrap_or(self.length), noise_w.unwrap_or(self.noise_w)])).map_err(|e| e.to_string())?;
        let sid = Tensor::from_array(([1usize], vec![speaker])).map_err(|e| e.to_string())?;
        let out = self.session.run(ort::inputs!["input" => input, "input_lengths" => lens, "scales" => scales, "sid" => sid]).map_err(|e| e.to_string())?;
        let (_, data) = out["output"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        Ok(data.to_vec())
    }
}
