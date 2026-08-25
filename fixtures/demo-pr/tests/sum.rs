use ledger::sum;

#[test]
fn sums_all_elements() {
    assert_eq!(sum(&[1, 2, 3]), 6);
}

#[test]
fn sums_single_element() {
    assert_eq!(sum(&[5]), 5);
}
