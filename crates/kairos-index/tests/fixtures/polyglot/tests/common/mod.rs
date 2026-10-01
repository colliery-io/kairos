//! A module that 2 test crates share. rust-analyzer 1.93.0 `scip` panics on
//! a file in 2 crates, so the index leaves the second crate out of the run.

pub fn helper() -> usize {
    polyglot::enqueue_all(&[7]).len()
}
