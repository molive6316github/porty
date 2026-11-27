fn main(){
    let a:Vec<String>=std::env::args().collect();
    let s=marbeat_core::audio::load_song(&a[1]).unwrap();
    if let Some(t)=a.get(2){
        let truth:Vec<f64>=serde_json::from_str(&std::fs::read_to_string(t).unwrap()).unwrap();
        let mut errs=vec![];
        for b in &s.beats{ let e=truth.iter().map(|x|b-x).min_by(|p,q|p.abs().partial_cmp(&q.abs()).unwrap()).unwrap(); errs.push(e); }
        let n=errs.len() as f64; let mean=errs.iter().sum::<f64>()/n; let mx=errs.iter().fold(0.0f64,|m,e|m.max(e.abs()));
        let late=errs.iter().filter(|e| e.abs()>0.05).count();
        println!("bpm {:.1} beats {} (truth {}) mean err {:+.0}ms max {:.0}ms, >50ms: {}  first {:+.0}ms last {:+.0}ms",s.bpm,s.beats.len(),truth.len(),mean*1000.0,mx*1000.0,late,errs[0]*1000.0,errs[errs.len()-1]*1000.0);
    } else { println!("bpm {:.1} beats {}",s.bpm,s.beats.len()); }
}
