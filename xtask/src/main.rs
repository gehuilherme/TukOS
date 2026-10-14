use std::process::ExitCode;

mod image;
mod ktest;
mod qemu;
mod util;

pub type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "uso: cargo xtask <build|run|ktest|test>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result: Result<()> = match args.first().map(String::as_str) {
        Some("build") => image::build(&[]).map(|esp| println!("ESP pronta em {}", esp.display())),
        Some("run") => image::build(&[]).and_then(|esp| qemu::run_interactive(&esp)),
        Some("ktest") => ktest::run_all(),
        Some("test") => util::run(std::process::Command::new("cargo").args(["test", "-p", "xtask", "-p", "arch-x86_64"])),
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
