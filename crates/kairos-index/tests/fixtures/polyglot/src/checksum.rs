//! A checksum of bytes. `src/legacy.rs` has an exact copy of `checksum`
//! and of `Checksum::value`, with other comments and whitespace
//! (COLLIERY-T-1857).

/// A running checksum.
pub struct Checksum {
    sum: u32,
}

impl Checksum {
    /// The checksum so far: a getter of one line.
    pub fn value(&self) -> u32 { self.sum }
}

/// The checksum of `data`: each byte is added to 31 times the sum so far.
pub fn checksum(data: &[u8]) -> Checksum {
    let mut sum: u32 = 0;
    for byte in data {
        sum = sum.wrapping_mul(31).wrapping_add(u32::from(*byte));
    }
    Checksum { sum }
}
