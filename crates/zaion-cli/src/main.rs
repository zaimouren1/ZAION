mod commands;
mod config;

use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(e) = commands::run(&args) {
        // CliError variants already format themselves with the right prefix
        // (e.g. "usage: ..." for caller errors, "error: ..." for runtime
        // failures). Do not re-add an "error: " here or we get
        // "error: error: provider error: ...".
        eprintln!("{}", e);
        process::exit(1);
    }
}
