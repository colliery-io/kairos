mod common;

#[test]
fn second_uses_the_helper() {
    let n = common::helper();
    assert_eq!(n, 1);
}
