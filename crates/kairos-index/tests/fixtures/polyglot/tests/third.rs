//! A test crate with a root function of the same name as one in
//! tests/first.rs. SCIP gives the 2 functions one symbol.

fn setup() -> usize {
    3
}

#[test]
fn third_uses_its_setup() {
    let n = setup();
    assert_eq!(n, 3);
}
