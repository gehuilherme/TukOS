//! Read-only embedded filesystem (initrd): normalized path -> baked bytes.
//!
//! Paths coming from user mode are normalized before lookup so that
//! `C:\Cat.TXT`, `c:/cat.txt` and `\??\C:\cat.txt` all name the same file, and
//! anything that could climb out of the table (`..`) is rejected outright. The
//! table is the only thing a lookup can ever reach: there is no host/disk path.

extern crate alloc;

use alloc::string::String;

/// (normalized path, contents). Keys must already be in normalized form:
/// lowercase, `/` separators, no drive, no leading slash.
const FILES: &[(&str, &[u8])] = &[
    (
        "cat.txt",
        include_bytes!("../../conformance/fileio/cat.txt"),
    ),
    // Conformance programs (empty when not built by `cargo xtask conformance`).
    (
        "parent.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/parent.exe")),
    ),
    (
        "child.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/child.exe")),
    ),
];

/// Normalizes `path` to the table's key form; `None` for paths that must never
/// resolve (traversal, embedded NUL, empty result, a drive other than `C:`).
pub fn normalize(path: &str) -> Option<String> {
    let mut s: String = path
        .chars()
        .map(|c| {
            if c == '\\' {
                '/'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();
    if s.contains('\0') {
        return None;
    }
    // Strip exactly one root prefix: NT `\??\c:\`, then a DOS `c:\` drive.
    if let Some(r) = s.strip_prefix("/??/") {
        s = r.into();
    }
    if let Some(r) = s.strip_prefix("c:") {
        s = r.into();
    }
    let mut out = String::new();
    for seg in s.split('/') {
        match seg {
            "" | "." => {}
            ".." => return None,
            _ => {
                if !out.is_empty() {
                    out.push('/');
                }
                out.push_str(seg);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Extra files registered at runtime by kernel self-tests (never in a real build).
#[cfg(feature = "ktest")]
static TEST_FILES: arch_x86_64::interrupts::IrqMutex<alloc::vec::Vec<(String, &'static [u8])>> =
    arch_x86_64::interrupts::IrqMutex::new(alloc::vec::Vec::new());

/// ktest only: makes `bytes` resolvable as `key` (already normalized form).
#[cfg(feature = "ktest")]
pub fn register_test_file(key: &str, bytes: &'static [u8]) {
    let mut g = TEST_FILES.lock();
    if !g.iter().any(|(k, _)| k == key) {
        g.push((String::from(key), bytes));
    }
}

/// Looks `path` up in the initrd.
pub fn lookup(path: &str) -> Option<&'static [u8]> {
    let key = normalize(path)?;
    if let Some((_, v)) = FILES.iter().find(|(k, _)| *k == key) {
        return Some(*v);
    }
    #[cfg(feature = "ktest")]
    if let Some((_, v)) = TEST_FILES.lock().iter().find(|(k, _)| *k == key) {
        return Some(*v);
    }
    None
}
