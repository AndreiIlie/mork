#![forbid(unsafe_code)]
fn main() {
    let spec = std::env::args().nth(1).and_then(|a| serde_json::from_str(&a).ok()).unwrap_or_default();
    println!("{}", mork_fakes::probe::report(&spec));
}
