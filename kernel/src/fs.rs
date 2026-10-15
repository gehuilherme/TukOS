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
const FILES: &[(&str, &[u8])] = &[(
    "cat.txt",
    include_bytes!("../../conformance/fileio/cat.txt"),
)];

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

/// Looks `path` up in the initrd.
pub fn lookup(path: &str) -> Option<&'static [u8]> {
    let key = normalize(path)?;
    FILES.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}
