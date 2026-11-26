use marbeat_ai::{piper::Piper, vc::*};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    marbeat_ai::init(&a[1]).unwrap();
    let d = "/home/user/porty/marbeat/ai";
    let mut p = Piper::load(&format!("{d}/piper/en_GB-vctk-medium.onnx"), &format!("{d}/piper/en_GB-vctk-medium.onnx.json")).unwrap();
    let t = std::time::Instant::now();
    let mut vc = Vc::load(&[format!("{d}/vc/wavlm_mm8.part00"), format!("{d}/vc/wavlm_mm8.part01")], &format!("{d}/vc/hifigan_knnvc.onnx")).unwrap();
    println!("loaded vc in {:?}", t.elapsed());
    let say = |p: &mut Piper, s: &str, spk: i64| { let ids = p.phoneme_ids(s).unwrap(); resample(&p.synth(&ids, spk, None, None, None).unwrap(), p.sample_rate, 16000) };
    let refa = say(&mut p, "The quick brown fox jumps over the lazy dog while the marbles roll down the neon track, and nobody knows who will win the race tonight.", 60);
    let src = say(&mut p, "hello world, i am a marble", 10);
    let t = std::time::Instant::now();
    let r = vc.reference(&refa).unwrap();
    println!("reference {:.1}s audio in {:?}", refa.len() as f32 / 16000.0, t.elapsed());
    let t = std::time::Instant::now();
    let out = vc.convert(&src, &r).unwrap();
    println!("converted {:.2}s -> {:.2}s in {:?}", src.len() as f32 / 16000.0, out.len() as f32 / 16000.0, t.elapsed());
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    println!("rms src {:.3} out {:.3} ref {:.3}", rms(&src), rms(&out), rms(&refa));
    for (n, x) in [("src", &src), ("out", &out), ("ref", &refa)] { let mut f = std::fs::File::create(format!("/tmp/vc_{n}.raw")).unwrap(); for s in x.iter() { std::io::Write::write_all(&mut f, &((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).unwrap(); } }
}
