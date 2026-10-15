/// ```
/// assert_eq!(testmulti::inc(1), 3, "doc says three");
/// ```
pub fn inc(x: i32) -> i32 { x + 1 }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inc_works() { println!("hello from inc_works"); assert_eq!(inc(1), 2); }
    #[test]
    #[ignore = "too slow"]
    fn slow() {}
    #[test]
    #[should_panic]
    fn should_have_panicked() { let _ = inc(1); }
    #[test]
    fn unwrap_none() { let v: Option<i32> = None; v.unwrap(); }
}
