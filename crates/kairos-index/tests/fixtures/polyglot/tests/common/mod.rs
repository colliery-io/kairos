//! A module that 3 test crates share. The pinned rust-analyzer indexes it
//! for each of them (COLLIERY-T-1858).

pub fn helper() -> usize {
    polyglot::enqueue_all(&[7]).len()
}
