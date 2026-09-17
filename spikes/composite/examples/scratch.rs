//! A scratch file: two things rust-analyzer flags on its own.
fn main() {
    let count: u32 = "twelve";
    let names = Vec::<String>::new();
    names.nosuch_method();
    println!("{count} {}", names.len());
}
