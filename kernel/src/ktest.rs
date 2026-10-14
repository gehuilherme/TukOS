//! In-kernel self-tests, built only with the `ktest` feature.

type TestFn = fn() -> Result<(), &'static str>;

const TESTS: &[(&str, TestFn)] = &[("limine_base_revision_supported", limine_base_revision_supported)];

fn limine_base_revision_supported() -> Result<(), &'static str> {
    if crate::BASE_REVISION.is_supported() { Ok(()) } else { Err("revisao base nao suportada") }
}

pub fn run_all() -> ! {
    let (mut passed, mut failed) = (0u32, 0u32);
    for (name, test) in TESTS {
        match test() {
            Ok(()) => {
                kprintln!("@@TUKOS-KTEST-PASS {name}");
                passed += 1;
            }
            Err(why) => {
                kprintln!("@@TUKOS-KTEST-FAIL {name} {why}");
                failed += 1;
            }
        }
    }
    kprintln!("@@TUKOS-KTEST-DONE passed={passed} failed={failed}");
    crate::qemu::exit(if failed == 0 { crate::qemu::ExitCode::Success } else { crate::qemu::ExitCode::Failure })
}
