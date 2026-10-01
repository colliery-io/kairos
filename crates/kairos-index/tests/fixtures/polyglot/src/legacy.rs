//! The old checksum, kept for old callers. The code is the same as in
//! `src/checksum.rs`; only the comments and the whitespace differ.

pub struct Checksum {
    sum: u32,
}

impl Checksum {
    pub fn value(&self) -> u32 { self.sum }
}

// Kept as it was in the first release.
pub fn checksum(data: &[u8]) -> Checksum {
    let mut sum: u32 = 0;

    for byte in data {
        // 31 is a small prime.
        sum = sum
            .wrapping_mul(31)
            .wrapping_add(u32::from(*byte));
    }

    Checksum { sum }
}
