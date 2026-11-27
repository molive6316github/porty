use marbeat_core::{sim::*, course::*};
fn main() {
    let beats: Vec<f64> = (0..70).map(|i| 0.4 + i as f64 * 60.0 / 128.0).collect();
    let bars: Vec<(f64, f32)> = (0..18).map(|i| (0.4 + i as f64 * 4.0 * 60.0 / 128.0, 0.8)).collect();
    let mut course = build_course(30.0 * 175.0, "compose-test", "neon");
    let sim = compose_sim(&mut course, 6, 12345, "compose-test", 90.0, None, &beats, &bars);
    let timed: Vec<&Seg> = course.segs.iter().filter(|s| s.t0 > -1e8).collect();
    println!("twin {:.2}  frames {}  bumpers placed {}  bumper events {}", sim.twin, sim.frames, timed.len(), sim.events.iter().filter(|e| e.kind == 1).count());
    for b in beats.iter().take(14).skip(5) {
        let near: Vec<String> = sim.events.iter().filter(|e| ((e.f as f64 / FPS) - b).abs() < 0.2).map(|e| format!("k{} m{} {:+.0}ms v{:.0}", e.kind, e.m, ((e.f as f64 / FPS) - b) * 1000.0, e.v)).collect();
        let f = (b * FPS) as usize;
        println!("beat {:5.2}: leader {} y {:.0}  events {:?}", b, sim.lead[f.min(sim.frames - 1)], sim.at(f.min(sim.frames - 1), sim.lead[f.min(sim.frames - 1)] as usize).1, near);
    }
}
