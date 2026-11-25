#![forbid(unsafe_code)]
fn main() {
    std::process::exit(factory_fakes::fake::main("notify-send"));
}
