pub fn double(x: i32) -> i32 { x * 2 }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doubles() { assert_eq!(double(2), 4); }
    #[test]
    fn broken() { println!("about to fail"); assert_eq!(double(2), 5, "double of two"); }
    #[test]
    fn panics() { let v: Vec<i32> = vec![]; let _ = v[3]; }
}
