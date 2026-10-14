//! Kernel self-test protocol: parsing serial markers and judging runs.

use crate::qemu::Outcome;

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub passed: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub done: Option<(u32, u32)>,
    pub panic: Option<String>,
}

const MARK: &str = "@@TUKOS-";

pub fn parse(serial: &str) -> Report {
    let mut r = Report::default();
    for line in serial.lines() {
        let line = line.trim_end_matches('\r');
        let Some(pos) = line.find(MARK) else { continue };
        let rest = &line[pos + MARK.len()..];
        if let Some(name) = rest.strip_prefix("KTEST-PASS ") {
            r.passed.push(name.trim().to_string());
        } else if let Some(body) = rest.strip_prefix("KTEST-FAIL ") {
            let (name, reason) = body.split_once(' ').unwrap_or((body, ""));
            r.failed.push((name.to_string(), reason.trim().to_string()));
        } else if let Some(body) = rest.strip_prefix("KTEST-DONE ") {
            r.done = parse_done(body);
        } else if let Some(msg) = rest.strip_prefix("PANIC ") {
            r.panic.get_or_insert_with(|| msg.trim().to_string());
        }
    }
    r
}

fn parse_done(body: &str) -> Option<(u32, u32)> {
    let mut passed = None;
    let mut failed = None;
    for kv in body.split_whitespace() {
        match kv.split_once('=') {
            Some(("passed", v)) => passed = v.parse().ok(),
            Some(("failed", v)) => failed = v.parse().ok(),
            _ => {}
        }
    }
    Some((passed?, failed?))
}

pub fn check_ktest(outcome: Outcome, report: &Report) -> crate::Result<()> {
    if let Some(p) = &report.panic {
        return Err(format!("panic no kernel: {p}"));
    }
    if outcome == Outcome::Timeout {
        return Err("timeout: o kernel não terminou os autotestes".into());
    }
    let Some((passed, failed)) = report.done else {
        return Err(format!(
            "marcador KTEST-DONE ausente (resultado do QEMU: {outcome:?})"
        ));
    };
    if failed > 0 || !report.failed.is_empty() {
        let names: Vec<_> = report
            .failed
            .iter()
            .map(|(n, why)| format!("{n} ({why})"))
            .collect();
        return Err(format!(
            "{failed} autoteste(s) falharam: {}",
            names.join(", ")
        ));
    }
    if passed == 0 {
        return Err("nenhum autoteste foi executado".into());
    }
    if outcome != Outcome::Success {
        return Err(format!(
            "QEMU terminou com {outcome:?} apesar de todos os testes passarem"
        ));
    }
    Ok(())
}

pub fn check_expected_panic(
    outcome: Outcome,
    report: &Report,
    must_contain: &[&str],
) -> crate::Result<()> {
    let Some(msg) = &report.panic else {
        return Err(format!(
            "panic esperado não ocorreu (resultado do QEMU: {outcome:?})"
        ));
    };
    if outcome != Outcome::Failure {
        return Err(format!(
            "panic deveria encerrar o QEMU com falha, mas foi {outcome:?}"
        ));
    }
    for needle in must_contain {
        if !msg.contains(needle) {
            return Err(format!("mensagem de panic não contém {needle:?}: {msg}"));
        }
    }
    Ok(())
}

use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(60);

/// Boots each self-test kernel variant and checks its serial output.
pub fn run_all() -> crate::Result<()> {
    run_variant("ktest", check_ktest)?;
    run_variant("ktest-panic", |o, r| {
        check_expected_panic(o, r, &["ktest-panic deliberado"])
    })?;
    run_variant("ktest-fault", |o, r| {
        check_expected_panic(o, r, &["PAGE_FAULT", "cr2=0x7fff00000000"])
    })?;
    println!("ktest: todas as variantes passaram");
    Ok(())
}

fn run_variant(
    feature: &str,
    check: impl Fn(Outcome, &Report) -> crate::Result<()>,
) -> crate::Result<()> {
    let esp = crate::image::build(&[feature])?;
    let (outcome, serial) = crate::qemu::run_captured(&esp, TIMEOUT)?;
    let report = parse(&serial);
    check(outcome, &report).map_err(|e| {
        format!(
            "variante {feature}: {e}
--- serial ---
{serial}"
        )
    })?;
    println!(
        "ktest: variante {feature} ok ({} teste(s))",
        report.passed.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_collects_pass_fail_done() {
        let serial = "\
@@TUKOS-KTEST-PASS gdt_selectors_loaded
@@TUKOS-KTEST-FAIL idt_loaded base errada
@@TUKOS-KTEST-DONE passed=1 failed=1
";
        let r = parse(serial);
        assert_eq!(r.passed, vec!["gdt_selectors_loaded".to_string()]);
        assert_eq!(
            r.failed,
            vec![("idt_loaded".to_string(), "base errada".to_string())]
        );
        assert_eq!(r.done, Some((1, 1)));
        assert_eq!(r.panic, None);
    }

    #[test]
    fn parse_ignores_noise_and_crlf() {
        let serial = "BdsDxe: loading Boot0001\r\n\x1b[2J\x1b[01;01H@@TUKOS-KTEST-PASS a\r\nlixo @@TUKOS-KTEST-DONE passed=1 failed=0\r\n";
        let r = parse(serial);
        assert_eq!(r.passed, vec!["a".to_string()]);
        assert_eq!(r.done, Some((1, 0)));
    }

    #[test]
    fn parse_captures_panic_line() {
        let r = parse("@@TUKOS-PANIC excecao 14 PAGE_FAULT cr2=0x7fff00000000\nr15: 0\n");
        assert_eq!(
            r.panic.as_deref(),
            Some("excecao 14 PAGE_FAULT cr2=0x7fff00000000")
        );
        assert_eq!(r.done, None);
    }

    #[test]
    fn check_ktest_accepts_clean_run() {
        let r = Report {
            passed: vec!["a".into()],
            done: Some((1, 0)),
            ..Default::default()
        };
        assert!(check_ktest(Outcome::Success, &r).is_ok());
    }

    #[test]
    fn check_ktest_rejects_failures_timeouts_and_missing_done() {
        let failed = Report {
            failed: vec![("a".into(), "x".into())],
            done: Some((0, 1)),
            ..Default::default()
        };
        assert!(check_ktest(Outcome::Failure, &failed).is_err());
        let ok_report = Report {
            passed: vec!["a".into()],
            done: Some((1, 0)),
            ..Default::default()
        };
        assert!(check_ktest(Outcome::Timeout, &ok_report).is_err());
        let no_done = Report {
            passed: vec!["a".into()],
            ..Default::default()
        };
        assert!(check_ktest(Outcome::Success, &no_done).is_err());
        let empty = Report {
            done: Some((0, 0)),
            ..Default::default()
        };
        assert!(check_ktest(Outcome::Success, &empty).is_err());
    }

    #[test]
    fn check_expected_panic_requires_failure_and_text() {
        let r = Report {
            panic: Some("excecao 14 PAGE_FAULT cr2=0x7fff00000000".into()),
            ..Default::default()
        };
        assert!(
            check_expected_panic(Outcome::Failure, &r, &["PAGE_FAULT", "cr2=0x7fff00000000"])
                .is_ok()
        );
        assert!(check_expected_panic(Outcome::Failure, &r, &["GENERAL_PROTECTION"]).is_err());
        assert!(check_expected_panic(Outcome::Success, &r, &["PAGE_FAULT"]).is_err());
        assert!(check_expected_panic(Outcome::Failure, &Report::default(), &["x"]).is_err());
    }
}
