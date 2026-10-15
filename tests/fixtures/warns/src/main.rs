use std::collections::HashMap;

fn unused_arg(x: i32) -> i32 { 3 }

fn main() {
    let unused = 5;
    let v: Vec<i32> = Vec::new();
    if v.len() == 0 { println!("empty"); }
    let n = 3;
    let m = n.clone();
    println!("{}", unused_arg(m));
}
