use marbeat_core::course::*;
use marbeat_core::sim::*;
use marbeat_core::rng::hash_seed;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (seed, map, n, len) = (a[1].as_str(), a[2].as_str(), a[3].parse::<usize>().unwrap(), a[4].parse::<f64>().unwrap());
    let c = build_course(len * 210.0, seed, map);
    let bumps = c.segs.iter().filter(|s| s.kind == Kind::Bump).count();
    let pegs = c.segs.iter().filter(|s| s.kind == Kind::Peg).count();
    let ramps = c.segs.iter().filter(|s| s.kind == Kind::Ramp).count();
    let sim = simulate(&c, n, &[], hash_seed(&format!("{seed}|n")), seed, len * 3.0, None);
    println!("{{\"segs\":{},\"pegs\":{pegs},\"ramps\":{ramps},\"bumps\":{bumps},\"spins\":{},\"finishY\":{:.3},\"winner\":{},\"twin\":{:.3},\"frames\":{}}}",
        c.segs.len(), c.spins.len(), c.finish_y, sim.winner, sim.twin, sim.frames);
}
