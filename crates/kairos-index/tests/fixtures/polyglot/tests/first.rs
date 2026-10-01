mod common;

#[test]
fn first_uses_the_helper() {
    assert_eq!(common::helper(), 1);
}

fn setup() -> usize {
    1
}

#[test]
fn first_uses_its_setup() {
    let n = setup();
    assert_eq!(n, 1);
}
