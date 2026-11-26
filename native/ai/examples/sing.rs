use marbeat_ai::{piper::Piper, vc::resample, whisper::Whisper, sing};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    marbeat_ai::init(&a[1]).unwrap();
    let d = "/home/user/porty/marbeat/ai";
    let mut p = Piper::load(&format!("{d}/piper/en_GB-vctk-medium.onnx"), &format!("{d}/piper/en_GB-vctk-medium.onnx.json")).unwrap();
    let mut w = Whisper::load(&format!("{d}/models/Xenova/whisper-tiny.en")).unwrap();
    let lyrics = "the marbles roll down the neon track\nwho is going to win this race tonight\nkeep rolling and never stop";
    // a "song vocal": the sentences spoken by one voice at known times
    let mut audio: Vec<f32> = vec![0.0; 16000];
    for s in lyrics.lines() { let ids = p.phoneme_ids(s).unwrap(); let x = resample(&p.synth(&ids, 12, None, None, None).unwrap(), p.sample_rate, 16000); audio.extend(x); audio.extend(vec![0.0; 24000]); }
    let heard = w.transcribe(&audio).unwrap();
    let lines = sing::align(lyrics, &heard, audio.len() as f64 / 16000.0);
    for l in &lines { println!("{:.2}-{:.2} {}", l.t0, l.t1, l.text); }
    let out = sing::sing(&mut p, &lines, audio.len() as f64 / 16000.0, 44100, &|t| if t < 4.0 { 10 } else { 70 }, |_, _| {}).unwrap();
    println!("sung {:.1}s rms {:.3}", out.len() as f32 / 44100.0, (out.iter().map(|v| v * v).sum::<f32>() / out.len() as f32).sqrt());
}
