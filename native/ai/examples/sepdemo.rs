use marbeat_ai::sep::Separator;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    marbeat_ai::init(&a[1]).unwrap();
    let (sr, mono, st) = marbeat_core::audio::decode(&a[2]).unwrap();
    assert_eq!(sr, 44100);
    let l: Vec<f32> = st.chunks(2).map(|c| c[0]).collect();
    let r: Vec<f32> = st.chunks(2).map(|c| c[1]).collect();
    let mut s = Separator::load("/home/user/porty/marbeat/ai/sep/vocals.onnx").unwrap();
    let t = std::time::Instant::now();
    let (vl, _vr) = s.vocals(&l, &r).unwrap();
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    { let mut f = std::fs::File::create("/tmp/stem.raw").unwrap(); for s in &vl { std::io::Write::write_all(&mut f, &((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).unwrap(); } }
    println!("{:.1}s audio separated in {:?}; rms mix {:.3} vocals {:.3}", mono.len() as f32 / sr as f32, t.elapsed(), rms(&l), rms(&vl));
}
