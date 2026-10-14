use std::process::ExitCode;

mod disk;
mod image;
mod ktest;
mod ntdll;
mod qemu;
mod util;
mod vmware;

pub type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "uso: cargo xtask <build|run|ktest|test|vmware|ntdll>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result: Result<()> = match args.first().map(String::as_str) {
        Some("build") => image::build(&[]).map(|esp| println!("ESP pronta em {}", esp.display())),
        Some("run") => image::build(&[]).and_then(|esp| qemu::run_interactive(&esp)),
        Some("vmware") => image::build(&[]).and_then(|esp| vmware::run(&esp)),
        Some("ktest") => ktest::run_all(),
        Some("ntdll") => ntdll::build().map(|p| println!("ntdll.dll em {}", p.display())),
        Some("test") => util::run(std::process::Command::new("cargo").args([
            "test",
            "-p",
            "xtask",
            "-p",
            "arch-x86_64",
            "-p",
            "mm",
        ])),
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
