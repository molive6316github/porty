use marbeat_core::render::*;
use marbeat_core::sim::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = a.get(1).map(|s| s.as_str()).unwrap_or("neon");
    let seed = a.get(2).map(|s| s.as_str()).unwrap_or("42");
    let out = a.get(3).map(|s| s.as_str()).unwrap_or("/tmp/mb");
    let n = 6;
    let beats: Vec<f64> = (0..400).map(|i| i as f64 * 0.5).collect();
    let tl = build_timeline(&RaceSetup { seed, map, n, len: 40.0, beats: &beats, plan: &[], winner: None });
    let cols = [[255, 90, 90], [90, 200, 255], [120, 255, 140], [255, 210, 80], [200, 120, 255], [255, 150, 210]];
    let names = ["Ruby", "Azure", "Mint", "Sunny", "Violet", "Rosie"];
    let chars: Vec<CharStyle> = (0..n).map(|i| CharStyle { name: names[i].into(), color: cols[i], label: "Piper voice".into() }).collect();
    let sc = Scene { tl: &tl, chars: &chars, beats: &beats, title: "Marbeat", pro_post: true };
    let mut r = Renderer::new(540, 960);
    let t0 = std::time::Instant::now();
    for (k, t) in [1.0, 12.0, 25.0, tl.win_time + 1.0].iter().enumerate() {
        r.draw(&sc, *t);
        r.pm.save_png(format!("{}{}.png", out, k)).unwrap();
    }
    eprintln!("4 frames in {:?}, win_time {:.1}", t0.elapsed(), tl.win_time);
}
