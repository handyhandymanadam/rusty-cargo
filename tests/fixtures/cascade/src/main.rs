struct Point { x: i32, y: i32 }

fn norm(p: &Pointt) -> i32 {
    p.x * p.x + p.y * p.y
}

fn dist(a: &Pointt, b: &Pointt) -> i32 {
    norm(a) - norm(b)
}

fn main() {
    let p = Point { x: 1, y: 2 };
    let total = undefined_total + 1;
    println!("{} {}", total, undefined_total);
    println!("{}", dist(&p, &p));
}
