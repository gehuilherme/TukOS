//! In-kernel self-tests, built only with the `ktest` feature.

type TestFn = fn() -> Result<(), &'static str>;

const TESTS: &[(&str, TestFn)] = &[
    ("limine_base_revision_supported", limine_base_revision_supported),
    ("gdt_selectors_loaded", gdt_selectors_loaded),
    ("tss_loaded", tss_loaded),
    ("idt_loaded", idt_loaded),
    ("breakpoint_returns", breakpoint_returns),
    ("fbcon_draws_glyph", fbcon_draws_glyph),
];

fn gdt_selectors_loaded() -> Result<(), &'static str> {
    use arch_x86_64::{cpu, gdt};
    if cpu::read_cs() != gdt::KERNEL_CS {
        return Err("CS diferente de KERNEL_CS");
    }
    if cpu::read_ss() != gdt::KERNEL_DS {
        return Err("SS diferente de KERNEL_DS");
    }
    Ok(())
}

fn tss_loaded() -> Result<(), &'static str> {
    if arch_x86_64::cpu::str() == arch_x86_64::gdt::TSS_SEL { Ok(()) } else { Err("TR diferente de TSS_SEL") }
}

fn idt_loaded() -> Result<(), &'static str> {
    let p = arch_x86_64::cpu::sidt();
    let (limit, base) = (p.limit, p.base);
    if limit != 256 * 16 - 1 {
        return Err("limite da IDT incorreto");
    }
    if base != arch_x86_64::idt::table_address() {
        return Err("base da IDT nao aponta para a tabela do kernel");
    }
    Ok(())
}

fn breakpoint_returns() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    let before = crate::trap::BREAKPOINTS.load(Ordering::SeqCst);
    arch_x86_64::cpu::int3();
    let after = crate::trap::BREAKPOINTS.load(Ordering::SeqCst);
    if after == before + 1 { Ok(()) } else { Err("handler de breakpoint nao contou a excecao") }
}

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

fn fbcon_draws_glyph() -> Result<(), &'static str> {
    let mut guard = crate::fbcon::FBCON.lock();
    let con = guard.as_mut().ok_or("sem framebuffer")?;
    let (w, h) = (crate::fbcon::FbCon::cell_width(), crate::fbcon::FbCon::cell_height());
    let cell_lit = |con: &crate::fbcon::FbCon| (0..h).any(|y| (0..w).any(|x| con.pixel(x, y) != 0));

    con.clear();
    if cell_lit(con) {
        return Err("celula deveria estar apagada apos clear");
    }
    con.write_str("#");
    if !cell_lit(con) {
        return Err("glifo nao foi desenhado");
    }
    con.clear();
    Ok(())
}
