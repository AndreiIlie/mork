// Answers as F-H1's runs do (1.10.1), as its scenario says.
#![forbid(unsafe_code)]
fn main() {
    std::process::exit(mork_fakes::fake::main("fake-checks"));
}
