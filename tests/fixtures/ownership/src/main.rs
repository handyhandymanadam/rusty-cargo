fn consume(s: String) -> usize { s.len() }

fn moved() {
    let s = String::from("x");
    let t = s;
    println!("{s} {t}");
}

fn moved_into_call() {
    let name = String::from("n");
    consume(name);
    println!("{}", name.len());
}

fn two_mut() {
    let mut v = vec![1];
    let a = &mut v;
    let b = &mut v;
    a.push(1);
    b.push(2);
}

fn dangling() -> usize {
    let r;
    {
        let x = String::from("short");
        r = &x;
    }
    r.len()
}

fn assign_borrowed() {
    let mut x = 5;
    let r = &x;
    x = 6;
    println!("{r}");
}

fn main() {
    moved();
    moved_into_call();
    two_mut();
    dangling();
    assign_borrowed();
}
