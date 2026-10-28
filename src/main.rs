//! `sor`: command-line entry point.

mod cli;

use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::run(std::env::args_os().skip(1)) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("sor: {e}");
            ExitCode::FAILURE
        }
    }
}
