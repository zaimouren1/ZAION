//! Integration tests exposing the deliberate defects.
use sandbox_svc::{format_item, parse_batch, process_batch, tally, validate_token};

#[test]
fn batch_sum_respects_cap() {
    let items = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    // cap=3 should sum only 1+2+3 = 6 (BUG-1: current code sums all = 55)
    assert_eq!(process_batch(items, 3), 6);
}

#[test]
fn valid_token_accepted() {
    // 32 hex chars starting with "zx" (BUG-2: code requires "zk")
    let token = "zx1234567890abcdef1234567890abcd";
    assert!(validate_token(token), "valid zx token rejected");
}

#[test]
fn invalid_prefix_rejected() {
    let token = "zk1234567890abcdef1234567890abcd";
    assert!(!validate_token(token), "zk prefix should be rejected");
}

#[test]
fn format_is_one_based() {
    // index 0 should render as "item 1" (BUG-3: current renders "item 0")
    assert_eq!(format_item(0, 42), "item 1: 42");
}

#[test]
fn parse_valid_batch() {
    let raw = r#"{"items": [1, 2, 3]}"#;
    assert_eq!(parse_batch(raw).unwrap(), vec![1, 2, 3]);
}

#[test]
fn tally_counts_and_sums() {
    let stats = tally(&[1, 2, 3]);
    assert_eq!(stats["count"], 3);
    assert_eq!(stats["sum"], 6);
}
