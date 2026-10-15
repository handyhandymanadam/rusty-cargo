fn level2(n: i32) -> i32 { inner::boom(n) }

fn level1(n: i32) -> i32 { level2(n) + 1 }

fn main() {
    println!("start");
    let v = level1(0);
    println!("{v}");
}

#[cfg(test)]
mod tests {
    #[test]
    fn explodes() { super::level1(0); }
    #[test]
    fn fine() { assert_eq!(super::level1(5), 6); }
}
