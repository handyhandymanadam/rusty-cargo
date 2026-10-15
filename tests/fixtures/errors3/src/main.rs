fn takes(n: i32) -> i32 { n + 1 }

fn mismatch() -> i32 {
    takes("seven")
}

fn borrow() {
    let mut v = vec![1, 2, 3];
    let first = &v[0];
    v.push(4);
    println!("{first}");
}

fn nosuch() -> i32 {
    missing_fn(1)
}

fn main() {
    mismatch();
    borrow();
    nosuch();
}
