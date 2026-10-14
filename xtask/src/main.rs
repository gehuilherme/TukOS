use std::process::ExitCode;

mod ktest;
mod qemu;

pub type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "uso: cargo xtask <build|run|ktest|test>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result: Result<()> = match args.first().map(String::as_str) {
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            ExitCode::FAILURE
        }
    }
}
