use marbeat_core::render::*;
use marbeat_core::sim::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = a.get(1).map(|s| s.as_str()).unwrap_or("neon");
    let seed = a.get(2).map(|s| s.as_str()).unwrap_or("42");
    let out = a.get(3).map(|s| s.as_str()).unwrap_or("/tmp/mb");
    let n = 6;
    if a.get(1).map(|s| s.as_str()) == Some("export") {
        // marbeat-cli export <song> <out.mp4> [map] [seed] [scale]
        let song = marbeat_core::audio::load_song(&a[2]).expect("load song");
        let (map, seed) = (a.get(4).map(|s| s.as_str()).unwrap_or("neon"), a.get(5).map(|s| s.as_str()).unwrap_or("42"));
        let scale: u32 = a.get(6).and_then(|s| s.parse().ok()).unwrap_or(1);
        let len = (song.mono.len() as f64 / song.sr as f64).min(300.0);
        let tl = build_timeline(&RaceSetup { seed, map, n, len, beats: &song.beats, plan: &[], winner: None, bars: &[], compose: false, crowd: 0 });
        let cols = [[255, 90, 90], [90, 200, 255], [120, 255, 140], [255, 210, 80], [200, 120, 255], [255, 150, 210]];
        let names = ["Ruby", "Azure", "Mint", "Sunny", "Violet", "Rosie"];
        let chars: Vec<CharStyle> = (0..n).map(|i| CharStyle { name: names[i].into(), color: cols[i], label: "Marble".into() }).collect();
        let sc = Scene { lyrics: &[], bars: &[], tl: &tl, chars: &chars, beats: &song.beats, title: "Marbeat", pro_post: true };
        let t0 = std::time::Instant::now();
        let r = marbeat_core::export::export_mp4(&sc, Some(&song), &a[3], scale, |f, t| { if f % 120 == 0 { eprintln!("{f}/{t}"); } true });
        eprintln!("{:?} in {:?}", r, t0.elapsed());
        return;
    }
    let beats: Vec<f64> = (0..400).map(|i| i as f64 * 0.5).collect();
    let tl = build_timeline(&RaceSetup { seed, map, n, len: 40.0, beats: &beats, plan: &[], winner: None, bars: &[], compose: false, crowd: 0 });
    let cols = [[255, 90, 90], [90, 200, 255], [120, 255, 140], [255, 210, 80], [200, 120, 255], [255, 150, 210]];
    let names = ["Ruby", "Azure", "Mint", "Sunny", "Violet", "Rosie"];
    let chars: Vec<CharStyle> = (0..n).map(|i| CharStyle { name: names[i].into(), color: cols[i], label: "Piper voice".into() }).collect();
    let sc = Scene { lyrics: &[], bars: &[], tl: &tl, chars: &chars, beats: &beats, title: "Marbeat", pro_post: true };
    let mut r = Renderer::new(540, 960);
    let t0 = std::time::Instant::now();
    for (k, t) in [1.0, 12.0, 25.0, tl.win_time + 1.0].iter().enumerate() {
        r.draw(&sc, *t);
        r.pm.save_png(format!("{}{}.png", out, k)).unwrap();
    }
    eprintln!("4 frames in {:?}, win_time {:.1}", t0.elapsed(), tl.win_time);
}
