pub fn double(x: i32) -> i32 { x * 2 }

/// ```
/// assert_eq!(testpass::double(3), 6);
/// ```
pub fn doc() {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doubles() { assert_eq!(double(2), 4); }
    #[test]
    #[ignore]
    fn slow() {}
}
