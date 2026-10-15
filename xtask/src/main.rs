use std::process::ExitCode;

mod conformance;
mod disk;
mod gdi32;
mod image;
mod kernel32;
mod ktest;
mod ntdll;
mod qemu;
mod util;
mod vmware;

pub type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "uso: cargo xtask <build|run|ktest|test|vmware|ntdll|kernel32|conformance>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result: Result<()> = match args.first().map(String::as_str) {
        Some("build") => image::build(&[]).map(|esp| println!("ESP pronta em {}", esp.display())),
        Some("run") => conformance::build_shell_programs()
            .and_then(|()| image::build(&["shell"]))
            .and_then(|esp| qemu::run_interactive(&esp)),
        Some("vmware") => conformance::build_shell_programs()
            .and_then(|()| image::build(&["shell"]))
            .and_then(|esp| vmware::run_interactive(&esp)),
        Some("ktest") => ktest::run_all(),
        Some("conformance") => conformance::run_conformance(),
        Some("ntdll") => ntdll::build().map(|p| println!("ntdll.dll em {}", p.display())),
        Some("kernel32") => kernel32::build().map(|p| println!("kernel32.dll em {}", p.display())),
        Some("test") => util::run(std::process::Command::new("cargo").args([
            "test",
            "-p",
            "xtask",
            "-p",
            "arch-x86_64",
            "-p",
            "mm",
            "-p",
            "pe",
            "-p",
            "kbd",
            "-p",
            "gfx",
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
