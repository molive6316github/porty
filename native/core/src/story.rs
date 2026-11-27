//! Story engine: a plain-text script format, an Undertale-style typewriter with per-character blips, and race beats.
//!
//! Script format (one thing per line, `//` starts a comment):
//!   @char Ruby color=#ff5a5a blip=square pitch=520 speed=1.0
//!   @race map=neon seed=ritual marbles=Ruby,Azure winner=Ruby len=40 title=The Ritual
//!   @wait 1.5
//!   Ruby: You came back. {p} I didn't think you would.
//!   * Narration lines start with a star.
//!   {p} inside text = a short pause, {winner} = whoever won the last race.
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Char { pub name: String, pub color: [u8; 3], pub blip: String, pub pitch: f32, pub speed: f32 }
#[derive(Clone, Debug, Default)]
pub struct RaceCfg { pub map: Option<String>, pub seed: Option<String>, pub marbles: Vec<String>, pub winner: Option<String>, pub len: Option<f32>, pub title: Option<String> }
#[derive(Clone, Debug)]
pub enum Beat { Say { who: String, text: String }, Narrate { text: String }, Race(RaceCfg), Wait(f32) }
#[derive(Clone, Debug, Default)]
pub struct Script { pub chars: Vec<Char>, pub beats: Vec<Beat>, pub errors: Vec<String> }

fn parse_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim_start_matches('#');
    if h.len() != 6 { return None; }
    Some([u8::from_str_radix(&h[0..2], 16).ok()?, u8::from_str_radix(&h[2..4], 16).ok()?, u8::from_str_radix(&h[4..6], 16).ok()?])
}
/// `key=value key2=value with spaces`: a value runs until the next ` word=`
fn kv(rest: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for tok in rest.split_whitespace() {
        if let Some((k, v)) = tok.split_once('=') { if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric()) { out.push((k.to_lowercase(), v.to_string())); continue; } }
        if let Some(last) = out.last_mut() { last.1.push(' '); last.1.push_str(tok); }
    }
    out
}

pub fn parse(src: &str) -> Script {
    let mut sc = Script::default();
    for (n, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') { continue; }
        let at = |m: &str| format!("line {}: {}", n + 1, m);
        if let Some(rest) = line.strip_prefix("@char") {
            let rest = rest.trim();
            let (name, props) = match rest.split_once(char::is_whitespace) { Some((a, b)) => (a, b), None => (rest, "") };
            if name.is_empty() { sc.errors.push(at("@char needs a name")); continue; }
            let mut c = Char { name: name.to_string(), color: [200, 200, 220], blip: "square".into(), pitch: 440.0, speed: 1.0 };
            for (k, v) in kv(props) {
                match k.as_str() {
                    "color" => match parse_color(&v) { Some(x) => c.color = x, None => sc.errors.push(at("bad color, use #rrggbb")) },
                    "blip" => c.blip = v.to_lowercase(),
                    "pitch" => c.pitch = v.parse().unwrap_or(440.0),
                    "speed" => c.speed = v.parse().unwrap_or(1.0),
                    _ => sc.errors.push(at(&format!("unknown @char option '{k}'"))),
                }
            }
            sc.chars.push(c);
        } else if let Some(rest) = line.strip_prefix("@race") {
            let mut r = RaceCfg::default();
            for (k, v) in kv(rest) {
                match k.as_str() {
                    "map" => r.map = Some(v),
                    "seed" => r.seed = Some(v),
                    "marbles" => r.marbles = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                    "winner" => r.winner = Some(v),
                    "len" => r.len = v.parse().ok(),
                    "title" => r.title = Some(v),
                    _ => sc.errors.push(at(&format!("unknown @race option '{k}'"))),
                }
            }
            sc.beats.push(Beat::Race(r));
        } else if let Some(rest) = line.strip_prefix("@wait") {
            sc.beats.push(Beat::Wait(rest.trim().parse().unwrap_or(1.0)));
        } else if line.starts_with('@') {
            sc.errors.push(at("unknown command"));
        } else if let Some(t) = line.strip_prefix('*') {
            sc.beats.push(Beat::Narrate { text: t.trim().to_string() });
        } else if let Some((who, text)) = line.split_once(':') {
            let who = who.trim();
            if who.contains(' ') && !sc.chars.iter().any(|c| c.name == who) { sc.errors.push(at("speaker names can't contain spaces unless declared with @char")); }
            sc.beats.push(Beat::Say { who: who.to_string(), text: text.trim().to_string() });
        } else { sc.errors.push(at("expected 'Name: text', '* narration' or an @command")); }
    }
    for b in &sc.beats { if let Beat::Say { who, .. } = b { if !sc.chars.iter().any(|c| &c.name == who) && !sc.errors.iter().any(|e| e.contains(&format!("'{who}'"))) { sc.errors.push(format!("'{who}' speaks but has no @char line")); } } }
    sc
}

#[derive(Clone, Debug)]
pub enum Ev { Blip(usize), StartRace(RaceCfg), Finished }

const PAUSE: char = '\u{1}';

pub struct Player {
    pub script: Script,
    pub idx: usize,
    chars: Vec<char>,
    pos: usize,
    budget: f32,
    blips: u32,
    cps: f32,
    speaker: Option<usize>,
    wait_left: f32,
    racing: bool,
    pub done: bool,
    pub vars: HashMap<String, String>,
}

impl Player {
    pub fn new(script: Script) -> (Self, Vec<Ev>) {
        let mut p = Player { script, idx: 0, chars: vec![], pos: 0, budget: 0.0, blips: 0, cps: 34.0, speaker: None, wait_left: 0.0, racing: false, done: false, vars: HashMap::new() };
        let ev = p.enter();
        (p, ev)
    }
    fn resolve(&self, t: &str) -> Vec<char> {
        let mut s = t.replace("{p}", &PAUSE.to_string());
        for (k, v) in &self.vars { s = s.replace(&format!("{{{k}}}"), v); }
        s.chars().collect()
    }
    fn enter(&mut self) -> Vec<Ev> {
        self.chars.clear(); self.pos = 0; self.budget = 0.0; self.blips = 0; self.speaker = None; self.racing = false;
        let Some(b) = self.script.beats.get(self.idx).cloned() else { self.done = true; return vec![Ev::Finished] };
        match b {
            Beat::Say { who, text } => {
                self.speaker = self.script.chars.iter().position(|c| c.name == who);
                self.cps = 34.0 * self.speaker.map(|i| self.script.chars[i].speed).unwrap_or(1.0);
                self.chars = self.resolve(&text);
                vec![]
            }
            Beat::Narrate { text } => { self.cps = 40.0; self.chars = self.resolve(&text); vec![] }
            Beat::Race(r) => { self.racing = true; vec![Ev::StartRace(r)] }
            Beat::Wait(s) => { self.wait_left = s; vec![] }
        }
    }
    fn shown_len(&self) -> usize { self.chars[..self.pos].iter().filter(|c| **c != PAUSE).count() }
    pub fn typing(&self) -> bool { self.pos < self.chars.len() }
    pub fn in_race(&self) -> bool { self.racing }
    /// (speaker, text so far, whole line is visible)
    pub fn view(&self) -> Option<(Option<&Char>, String, bool)> {
        match self.script.beats.get(self.idx)? { Beat::Say { .. } | Beat::Narrate { .. } => {
            let s: String = self.chars[..self.pos].iter().filter(|c| **c != PAUSE).collect();
            Some((self.speaker.map(|i| &self.script.chars[i]), s, !self.typing()))
        } _ => None }
    }
    pub fn full_text(&self) -> String { self.chars.iter().filter(|c| **c != PAUSE).collect() }
    pub fn update(&mut self, dt: f32) -> Vec<Ev> {
        let mut ev = vec![];
        if self.done { return ev; }
        if let Some(Beat::Wait(_)) = self.script.beats.get(self.idx) {
            self.wait_left -= dt;
            if self.wait_left <= 0.0 { self.idx += 1; ev.extend(self.enter()); }
            return ev;
        }
        self.budget += dt;
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            let prev = if self.pos > 0 { self.chars[self.pos - 1] } else { ' ' };
            let extra = match prev { ',' => 0.10, '.' | '!' | '?' | '…' => 0.26, _ => 0.0 };
            let cost = if c == PAUSE { 0.35 } else { 1.0 / self.cps + extra };
            if self.budget < cost { break; }
            self.budget -= cost;
            self.pos += 1;
            if c != PAUSE && !c.is_whitespace() { self.blips += 1; if self.blips % 2 == 1 { if let Some(s) = self.speaker { ev.push(Ev::Blip(s)); } else { ev.push(Ev::Blip(usize::MAX)); } } }
        }
        ev
    }
    /// Space / Enter / click: finish the line, or go to the next beat.
    pub fn advance(&mut self) -> Vec<Ev> {
        if self.done || self.racing { return vec![]; }
        if matches!(self.script.beats.get(self.idx), Some(Beat::Wait(_))) { return vec![]; }
        if self.typing() { self.pos = self.chars.len(); return vec![]; }
        self.idx += 1;
        self.enter()
    }
    pub fn race_finished(&mut self, winner: &str) -> Vec<Ev> {
        self.vars.insert("winner".into(), winner.to_string());
        self.racing = false;
        self.idx += 1;
        self.enter()
    }
    pub fn progress(&self) -> (usize, usize) { (self.idx.min(self.script.beats.len()), self.script.beats.len()) }
}

/// A short "text blip" sound for a character: 22.05 kHz mono. `n` varies the pitch slightly so it doesn't drone.
pub fn blip(kind: &str, pitch: f32, n: u32) -> Vec<f32> {
    let sr = 22050.0f32;
    let len = (0.055 * sr) as usize;
    let jitter = 1.0 + (((n.wrapping_mul(2654435761)) >> 24) as f32 / 255.0 - 0.5) * 0.14;
    let f = pitch * jitter;
    let mut noise = 0x1234_5678u32 ^ n.wrapping_mul(97);
    let mut ph = 0.0f32;
    (0..len).map(|i| {
        let t = i as f32 / len as f32;
        let env = (1.0 - t).powf(1.6) * (i as f32 / 40.0).min(1.0);
        ph += f / sr;
        let x = ph.fract();
        let w = match kind {
            "triangle" => 4.0 * (x - 0.5).abs() - 1.0,
            "saw" => 2.0 * x - 1.0,
            "sine" => (x * std::f32::consts::TAU).sin(),
            "noise" => { noise ^= noise << 13; noise ^= noise >> 17; noise ^= noise << 5; (noise as f32 / u32::MAX as f32) * 2.0 - 1.0 }
            "pulse" => if x < 0.25 { 1.0 } else { -1.0 },
            _ => if x < 0.5 { 1.0 } else { -1.0 },
        };
        w * env * 0.22
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    const S: &str = "@char Ruby color=#ff5a5a blip=square pitch=520\n@char Azure color=#5ac8ff blip=triangle\nRuby: Hi. {p} You came.\n* The track hums.\n@race map=neon marbles=Ruby,Azure winner=Ruby\nAzure: {winner} won.\n";
    #[test]
    fn parses() { let s = parse(S); assert!(s.errors.is_empty(), "{:?}", s.errors); assert_eq!(s.chars.len(), 2); assert_eq!(s.beats.len(), 4); }
    #[test]
    fn plays_through() {
        let (mut p, _) = Player::new(parse(S));
        let mut blips = 0;
        for _ in 0..400 { for e in p.update(0.02) { if matches!(e, Ev::Blip(_)) { blips += 1; } } }
        assert!(blips > 3); assert!(!p.typing());
        p.advance(); assert!(p.view().unwrap().0.is_none()); // narration has no speaker
        assert!(p.advance().is_empty()); assert!(!p.typing()); // first press completes the line
        let ev = p.advance(); assert!(matches!(ev[0], Ev::StartRace(_)));
        assert!(p.advance().is_empty()); // can't skip a race
        p.race_finished("Ruby");
        for _ in 0..400 { p.update(0.02); }
        assert_eq!(p.full_text(), "Ruby won.");
    }
    #[test]
    fn reports_errors() { let s = parse("Nobody: hi\n@bogus\n"); assert_eq!(s.errors.len(), 2, "{:?}", s.errors); }
}
