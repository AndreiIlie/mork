#![forbid(unsafe_code)]

mod cli;
mod ctl;
mod launcher;
mod local;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(cli::run(&args));
}
