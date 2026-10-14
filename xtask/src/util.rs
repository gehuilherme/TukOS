use std::path::PathBuf;
use std::process::Command;

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

pub fn run(cmd: &mut Command) -> crate::Result<()> {
    let status = cmd.status().map_err(|e| format!("falha ao executar {cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} terminou com {status}")) }
}
