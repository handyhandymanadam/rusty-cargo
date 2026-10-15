pub fn double(x: i32) -> i32 { x * 2 }

pub fn boom(x: i32) -> i32 { if x > 0 { panic!("boom in library code") } else { x } }

/// Adds one.
///
/// ```
/// assert_eq!(docfail::inc(1), 2);
/// ```
pub fn inc(x: i32) -> i32 { x + 1 }

/// Triples. This doc test fails on purpose.
///
/// ```
/// let a = docfail::triple(2);
/// assert_eq!(a, 7, "triple of two");
/// ```
pub fn triple(x: i32) -> i32 { x * 3 }

/// Calls the library function that panics.
///
/// ```
/// let v = docfail::boom(1);
/// assert_eq!(v, 0);
/// ```
pub fn caller() {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doubles() { assert_eq!(double(2), 4); }
}
