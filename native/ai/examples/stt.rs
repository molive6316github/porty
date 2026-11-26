use marbeat_ai::{piper::Piper, whisper::Whisper, vc::resample};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    marbeat_ai::init(&a[1]).unwrap();
    let d = "/home/user/porty/marbeat/ai";
    let mut p = Piper::load(&format!("{d}/piper/en_GB-vctk-medium.onnx"), &format!("{d}/piper/en_GB-vctk-medium.onnx.json")).unwrap();
    let mut w = Whisper::load(&format!("{d}/models/Xenova/whisper-tiny.en")).unwrap();
    // test: three sentences with a 1.5 s silence gap in the middle, so true timings are known
    let mut audio: Vec<f32> = vec![0.0; 16000];
    let mut truth = vec![];
    for s in ["the marbles roll down the neon track", "who is going to win this race tonight", "keep rolling and never stop"] {
        let ids = p.phoneme_ids(s).unwrap();
        let x = resample(&p.synth(&ids, 12, None, None, None).unwrap(), p.sample_rate, 16000);
        truth.push((s, audio.len() as f64 / 16000.0, (audio.len() + x.len()) as f64 / 16000.0));
        audio.extend(x);
        audio.extend(vec![0.0; 24000]);
    }
    let t = std::time::Instant::now();
    let words = w.transcribe(&audio).unwrap();
    println!("transcribed {:.1}s in {:?}", audio.len() as f32 / 16000.0, t.elapsed());
    for x in &words { println!("{:6.2} {:6.2} {}", x.start, x.end, x.text); }
    for (s, a, b) in truth { println!("TRUTH {a:.2}-{b:.2}: {s}"); }
}
