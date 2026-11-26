//! The AI pipeline: fetch models once, then separate vocals -> hear the lyrics -> sync -> sing in the leader's voice -> remix.
use marbeat_ai::{piper::Piper, sep::Separator, sing, vc::{resample, Vc}, whisper::Whisper};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const BASE: &str = "https://assets.molive6316.com/marbeat/";

pub fn ai_dir() -> PathBuf {
    if let Ok(d) = std::env::var("MARBEAT_AI_DIR") { return d.into(); }
    let b = std::env::var("LOCALAPPDATA").map(PathBuf::from).or_else(|_| std::env::var("HOME").map(|h| Path::new(&h).join(".local/share"))).unwrap_or_else(|_| ".".into());
    b.join("Marbeat").join("data")
}

fn ort_name() -> &'static str { if cfg!(windows) { "onnxruntime.dll" } else { "libonnxruntime.so" } }

fn fetch(rel: &str, dest: &Path, status: &dyn Fn(String)) -> Result<(), String> {
    if dest.exists() && dest.metadata().map(|m| m.len() > 0).unwrap_or(false) { return Ok(()); }
    if let Some(d) = dest.parent() { std::fs::create_dir_all(d).map_err(|e| e.to_string())?; }
    let url = format!("{BASE}{rel}");
    let resp = ureq::get(&url).call().map_err(|e| format!("download {rel}: {e}"))?;
    let total: u64 = resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(0);
    let part = dest.with_extension("part");
    let mut f = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut r = resp.into_reader();
    let (mut buf, mut got) = (vec![0u8; 1 << 17], 0u64);
    loop {
        let n = r.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        status(format!("Downloading {} {}", rel.rsplit('/').next().unwrap_or(rel), if total > 0 { format!("{}%", got * 100 / total) } else { format!("{} MB", got / 1_000_000) }));
    }
    drop(f);
    std::fs::rename(&part, dest).map_err(|e| e.to_string())
}

pub struct Models { pub dir: PathBuf }
const CORE: &[&str] = &[
    "ai/piper/en_GB-vctk-medium.onnx", "ai/piper/en_GB-vctk-medium.onnx.json",
    "ai/models/Xenova/whisper-tiny.en/onnx/encoder_model_quantized.onnx", "ai/models/Xenova/whisper-tiny.en/onnx/decoder_model_merged_quantized.onnx",
    "ai/models/Xenova/whisper-tiny.en/tokenizer.json", "ai/models/Xenova/whisper-tiny.en/generation_config.json",
    "ai/sep/vocals.onnx",
];
const CLONE: &[&str] = &["ai/vc/wavlm_mm8.part00", "ai/vc/wavlm_mm8.part01", "ai/vc/hifigan_knnvc.onnx"];

/// Makes sure the runtime and models exist (downloading on first use) and starts ONNX Runtime.
pub fn ensure(need_clone: bool, status: &dyn Fn(String)) -> Result<Models, String> {
    let dir = ai_dir();
    // a local checkout of the web files works too (MARBEAT_AI_DIR points at the marbeat/ folder)
    let local = std::env::var("MARBEAT_AI_DIR").is_ok();
    if !local {
        for rel in CORE.iter().chain(if need_clone { CLONE.iter() } else { [].iter() }) { fetch(rel, &dir.join(rel), status)?; }
    }
    let lib = if let Ok(p) = std::env::var("ORT_DYLIB_PATH") { PathBuf::from(p) } else {
        let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(ort_name())));
        match beside { Some(b) if b.exists() => b, _ => { let p = dir.join(ort_name()); fetch(&format!("native/{}", ort_name()), &p, status)?; p } }
    };
    marbeat_ai::init(&lib.to_string_lossy())?;
    Ok(Models { dir })
}

pub struct Voice { pub speaker: i64, pub clone: Option<String> }

pub struct Result_ { pub mixed: Vec<f32>, pub lines: usize, pub heard: usize }

fn load_wav_16k(path: &str) -> Result<Vec<f32>, String> {
    let (sr, mono, _) = marbeat_core::audio::decode(path)?;
    Ok(resample(&mono, sr, 16000))
}

/// stereo interleaved at `sr`; `leader_at(t)` gives the roster index leading at song time t
pub fn run(stereo: &[f32], sr: u32, lyrics: &str, voices: &[Voice], leader_at: &dyn Fn(f64) -> usize, status: &dyn Fn(String)) -> Result<Result_, String> {
    let need_clone = voices.iter().any(|v| v.clone.is_some());
    let m = ensure(need_clone, status)?;
    let root = if std::env::var("MARBEAT_AI_DIR").is_ok() { m.dir.clone() } else { m.dir.clone() };
    let p = |rel: &str| root.join("ai").join(rel).to_string_lossy().to_string();
    // 1. separate vocals at 44.1 kHz
    status("Separating vocals from the music…".into());
    let l0: Vec<f32> = stereo.chunks(2).map(|c| c[0]).collect();
    let r0: Vec<f32> = stereo.chunks(2).map(|c| c[1]).collect();
    let (l, r) = if sr == 44100 { (l0.clone(), r0.clone()) } else { (resample(&l0, sr, 44100), resample(&r0, sr, 44100)) };
    let mut sep = Separator::load(&p("sep/vocals.onnx"))?;
    let (vl, vr) = sep.vocals(&l, &r)?;
    drop(sep);
    let vmono: Vec<f32> = vl.iter().zip(&vr).map(|(a, b)| (a + b) * 0.5).collect();
    let v16 = resample(&vmono, 44100, 16000);
    // 2. what is being sung
    status("Listening to the lyrics…".into());
    let mut wh = Whisper::load(&p("models/Xenova/whisper-tiny.en"))?;
    let heard = wh.transcribe(&v16)?;
    drop(wh);
    // 3. sync the user's lyrics to it
    let total = l.len() as f64 / 44100.0;
    let mut lines = sing::align(lyrics, &heard, total);
    sing::refine_starts(&mut lines, &v16);
    // the vocal stem's soft consonants register a little late; start singing slightly ahead so words land on the beat
    for l in &mut lines { l.t0 = (l.t0 - 0.1).max(0.0); l.t1 = (l.t1 - 0.1).max(l.t0 + 0.3); for w in &mut l.words { w.1 = (w.1 - 0.1).max(0.0); } }
    if lines.is_empty() { return Err("no lyrics to sing".into()); }
    if std::env::var("MARBEAT_DEBUG").is_ok() { for l in &lines { eprintln!("line {:.2}-{:.2} {}", l.t0, l.t1, l.text); } }
    // 4. sing every line in the voice of whoever leads at that moment
    let mut piper = Piper::load(&p("piper/en_GB-vctk-medium.onnx"), &p("piper/en_GB-vctk-medium.onnx.json"))?;
    let mut vc = if need_clone { Some(Vc::load(&[p("vc/wavlm_mm8.part00"), p("vc/wavlm_mm8.part01")], &p("vc/hifigan_knnvc.onnx"))?) } else { None };
    let mut refs: Vec<Option<marbeat_ai::vc::Reference>> = vec![];
    for v in voices { refs.push(match (&v.clone, vc.as_mut()) { (Some(path), Some(vc)) => { status("Learning a cloned voice…".into()); Some(vc.reference(&load_wav_16k(path)?)?) } _ => None }); }
    let mut sung = vec![0f32; l.len() + 44100];
    let n = lines.len();
    for (i, line) in lines.iter().enumerate() {
        status(format!("Singing line {}/{}…", i + 1, n));
        let who = leader_at(line.t0).min(voices.len().saturating_sub(1));
        let one = [line.clone()];
        let audio = sing::sing(&mut piper, &one, total, 16000, &|_| voices[who].speaker, |_, _| {})?;
        let audio = match (&refs[who], vc.as_mut()) {
            (Some(rf), Some(vc)) => { let s0 = (line.t0 * 16000.0) as usize; let e0 = ((line.t1 + 0.3) * 16000.0).min(audio.len() as f64) as usize; let seg = &audio[s0.min(e0)..e0]; let conv = vc.convert(seg, rf)?; let mut full = vec![0f32; audio.len()]; for (k, v) in conv.iter().enumerate() { if s0 + k < full.len() { full[s0 + k] = *v; } } full }
            _ => audio,
        };
        let up = resample(&audio, 16000, 44100);
        for (k, v) in up.iter().enumerate() { if k < sung.len() { sung[k] += v; } }
    }
    // 5. remix: original music without vocals + the marbles' voices
    status("Mixing…".into());
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt();
    let target = (rms(&vmono) * 1.2).clamp(0.05, 0.2);
    let g = target / rms(&sung).max(1e-4);
    let (mut ml, mut mr): (Vec<f32>, Vec<f32>) = (vec![0.0; l.len()], vec![0.0; l.len()]);
    for i in 0..l.len() { ml[i] = (l[i] - vl[i]) + sung[i] * g; mr[i] = (r[i] - vr[i]) + sung[i] * g; }
    let peak = ml.iter().chain(&mr).fold(0f32, |a, b| a.max(b.abs()));
    let k = if peak > 0.97 { 0.97 / peak } else { 1.0 };
    let (ml, mr) = if sr == 44100 { (ml, mr) } else { (resample(&ml, 44100, sr), resample(&mr, 44100, sr)) };
    let mut mixed = Vec::with_capacity(ml.len() * 2);
    for i in 0..ml.len() { mixed.push(ml[i] * k); mixed.push(mr[i] * k); }
    Ok(Result_ { mixed, lines: n, heard: heard.len() })
}
