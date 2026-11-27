//! The same random generator and seed hash as the web version, so a seed means the same race.

/// mulberry32, bit for bit like the JavaScript version.
#[derive(Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(if seed == 0 { 1 } else { seed })
    }
    pub fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6D2B79F5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        ((t ^ (t >> 14)) as f64) / 4294967296.0
    }
}

/// FNV-1a over UTF-16 code units (what the JS `charCodeAt` loop does).
pub fn hash_seed(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for ch in s.chars() {
        let mut buf = [0u16; 2];
        let unit = ch.encode_utf16(&mut buf)[0] as u32;
        h ^= unit;
        h = h.wrapping_mul(16777619);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_the_javascript_version() {
        // reference values produced by the web version's rng()/hashSeed()
        assert_eq!(hash_seed("42"), 2279835011);
        assert_eq!(hash_seed("42|course|neon"), 957685516);
        assert_eq!(hash_seed("cup✨1"), 2445039164);
        let mut g = Rng::new(hash_seed("42|course|neon"));
        let want = [0.01898323348723352, 0.12722088349983096, 0.07313639484345913, 0.01288175699301064, 0.9516419766005129, 0.09938686271198094];
        for w in want {
            assert!((g.next() - w).abs() < 1e-15, "rng drifted from the JS version");
        }
    }
}
