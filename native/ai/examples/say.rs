fn main() {
    let a: Vec<String> = std::env::args().collect();
    marbeat_ai::init(&a[1]).unwrap();
    let d = &a[2];
    let mut p = marbeat_ai::piper::Piper::load(&format!("{d}/en_GB-vctk-medium.onnx"), &format!("{d}/en_GB-vctk-medium.onnx.json")).unwrap();
    let text = &a[3];
    let ids = p.phoneme_ids(text).unwrap();
    println!("ids {:?}", ids);
    let t = std::time::Instant::now();
    let w = p.synth(&ids, a.get(4).and_then(|s| s.parse().ok()).unwrap_or(10), None, None, None).unwrap();
    println!("{} samples ({:.2}s) in {:?}", w.len(), w.len() as f32 / p.sample_rate as f32, t.elapsed());
    let mut f = std::fs::File::create("/tmp/say.raw").unwrap();
    for s in w { std::io::Write::write_all(&mut f, &((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).unwrap(); }
}
