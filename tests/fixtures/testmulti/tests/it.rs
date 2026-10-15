#[test]
fn integration_ok() { assert_eq!(testmulti::inc(2), 3); }

#[test]
fn integration_assert() { assert!(1 + 1 == 3); }

#[test]
fn integration_message() { panic!("nope: {}", 42); }
