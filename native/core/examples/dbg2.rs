use marbeat_core::{sim::*, course::*};
fn main() {
    let beats: Vec<f64> = (0..70).map(|i| 0.4 + i as f64 * 60.0 / 128.0).collect();
    let bars: Vec<(f64, f32)> = (0..18).map(|i| (0.4 + i as f64 * 4.0 * 60.0 / 128.0, 0.8)).collect();
    for map in ["neon", "slide", "pachinko"] { for hc in [1200.0, 1800.0, 2600.0, 3600.0, 5250.0] {
        let mut course = build_course(hc, "compose-test", map);
        let sim = compose_sim(&mut course, 6, 12345, "compose-test", 90.0, None, &beats, &bars);
        let plain = simulate(&build_course(hc, "compose-test", map), 6, &[], 12345, "compose-test", 90.0, None);
        println!("{map:8} hc {hc:6.0}: finish with bumpers {:5.1}s, without {:5.1}s, finish_y {:.0}", sim.twin, plain.twin, course.finish_y);
    } }
}
