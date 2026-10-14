//! In-kernel self-tests, built only with the `ktest` feature.

extern crate alloc;

use core::sync::atomic::{AtomicU64, Ordering};

type TestFn = fn() -> Result<(), &'static str>;

const TESTS: &[(&str, TestFn)] = &[
    (
        "limine_base_revision_supported",
        limine_base_revision_supported,
    ),
    ("gdt_selectors_loaded", gdt_selectors_loaded),
    ("tss_loaded", tss_loaded),
    ("idt_loaded", idt_loaded),
    ("breakpoint_returns", breakpoint_returns),
    ("fbcon_draws_glyph", fbcon_draws_glyph),
    ("frame_alloc_roundtrip", frame_alloc_roundtrip),
    ("map_translate_unmap", map_translate_unmap),
    ("map_creates_missing_tables", map_creates_missing_tables),
    ("heap_box_and_vec", heap_box_and_vec),
    ("ist_stacks_installed", ist_stacks_installed),
    ("timer_ticks_advance", timer_ticks_advance),
    ("spawned_thread_runs", spawned_thread_runs),
    ("yield_alternates_threads", yield_alternates_threads),
    ("preemption_runs_both_threads", preemption_runs_both_threads),
];

fn ist_stacks_installed() -> Result<(), &'static str> {
    use arch_x86_64::{gdt, idt};
    let tops = gdt::ist_tops();
    if tops.iter().any(|&t| t == 0 || t % 16 != 0) {
        return Err("topo de pilha IST nulo ou desalinhado");
    }
    if tops[0] == tops[1] || tops[1] == tops[2] || tops[0] == tops[2] {
        return Err("pilhas IST compartilhadas");
    }
    if idt::entry(8).ist != gdt::IST_DOUBLE_FAULT
        || idt::entry(2).ist != gdt::IST_NMI
        || idt::entry(18).ist != gdt::IST_MACHINE_CHECK
    {
        return Err("vetores 8/2/18 nao apontam para a IST correta");
    }
    Ok(())
}

fn heap_box_and_vec() -> Result<(), &'static str> {
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    let b = Box::new(0xDEAD_BEEF_u64);
    if *b != 0xDEAD_BEEF {
        return Err("Box nao preservou o valor");
    }
    let mut v: Vec<u64> = Vec::new();
    for i in 0..1000 {
        v.push(i);
    }
    if v.iter().sum::<u64>() != (0..1000).sum() {
        return Err("Vec corrompeu dados ao crescer");
    }
    Ok(())
}

fn map_translate_unmap() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let phys = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    // A free slot in the non-canonical-safe test window well above the kernel heap.
    let virt = 0xffff_f000_0000_0000u64;
    let flags = PageFlags::PRESENT
        .union(PageFlags::WRITABLE)
        .union(PageFlags::NO_EXECUTE);
    unsafe { crate::mem::paging::map_page(virt, phys, flags).map_err(|_| "map falhou")? };
    if crate::mem::paging::translate(virt) != Some(phys) {
        return Err("translate nao devolveu o frame mapeado");
    }
    unsafe {
        core::ptr::write_volatile(virt as *mut u64, 0x1122_3344_5566_7788);
        if core::ptr::read_volatile(virt as *const u64) != 0x1122_3344_5566_7788 {
            return Err("pagina mapeada nao e gravavel");
        }
    }
    unsafe { crate::mem::paging::unmap_page(virt).map_err(|_| "unmap falhou")? };
    if crate::mem::paging::translate(virt).is_some() {
        return Err("translate ainda resolve apos unmap");
    }
    crate::mem::frames::free_frame(phys);
    Ok(())
}

fn map_creates_missing_tables() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let before = crate::mem::frames::free_frame_count();
    let phys = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    // An address whose P3/P2/P1 tables certainly do not exist yet.
    let virt = 0xffff_e800_0000_0000u64;
    unsafe {
        crate::mem::paging::map_page(virt, phys, PageFlags::PRESENT.union(PageFlags::WRITABLE))
            .map_err(|_| "map falhou ao criar tabelas")?;
    }
    if crate::mem::paging::translate(virt) != Some(phys) {
        return Err("tabelas intermediarias nao foram criadas corretamente");
    }
    unsafe { crate::mem::paging::unmap_page(virt).map_err(|_| "unmap falhou")? };
    crate::mem::frames::free_frame(phys);
    // map consumed frames for the intermediate tables; we don't require them back,
    // but the count must not have grown.
    if crate::mem::frames::free_frame_count() > before {
        return Err("contagem de frames livres cresceu inesperadamente");
    }
    Ok(())
}

fn frame_alloc_roundtrip() -> Result<(), &'static str> {
    let before = crate::mem::frames::free_frame_count();
    if before == 0 {
        return Err("nenhum frame livre apos init");
    }
    let hhdm = crate::mem::frames::hhdm_offset();
    let f1 = crate::mem::frames::alloc_frame().ok_or("alloc_frame devolveu None")?;
    let f2 = crate::mem::frames::alloc_frame().ok_or("alloc_frame devolveu None")?;
    if f1 == f2 {
        return Err("alloc_frame devolveu o mesmo frame duas vezes");
    }
    if f1 % crate::mem::frames::FRAME_SIZE != 0 || f2 % crate::mem::frames::FRAME_SIZE != 0 {
        return Err("frame nao alinhado a 4096");
    }
    // The frame must be writable through the HHDM.
    let ptr = (f1 + hhdm) as *mut u64;
    unsafe {
        core::ptr::write_volatile(ptr, 0xA5A5_A5A5_DEAD_BEEF);
        if core::ptr::read_volatile(ptr) != 0xA5A5_A5A5_DEAD_BEEF {
            return Err("frame nao e gravavel via HHDM");
        }
    }
    crate::mem::frames::free_frame(f1);
    crate::mem::frames::free_frame(f2);
    if crate::mem::frames::free_frame_count() != before {
        return Err("contagem de frames livres nao voltou ao valor inicial");
    }
    Ok(())
}

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
    if arch_x86_64::cpu::str() == arch_x86_64::gdt::TSS_SEL {
        Ok(())
    } else {
        Err("TR diferente de TSS_SEL")
    }
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
    let before = crate::trap::BREAKPOINTS.load(Ordering::SeqCst);
    arch_x86_64::cpu::int3();
    let after = crate::trap::BREAKPOINTS.load(Ordering::SeqCst);
    if after == before + 1 {
        Ok(())
    } else {
        Err("handler de breakpoint nao contou a excecao")
    }
}

fn limine_base_revision_supported() -> Result<(), &'static str> {
    if crate::BASE_REVISION.is_supported() {
        Ok(())
    } else {
        Err("revisao base nao suportada")
    }
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
    crate::qemu::exit(if failed == 0 {
        crate::qemu::ExitCode::Success
    } else {
        crate::qemu::ExitCode::Failure
    })
}

fn fbcon_draws_glyph() -> Result<(), &'static str> {
    let mut guard = crate::fbcon::FBCON.lock();
    let con = guard.as_mut().ok_or("sem framebuffer")?;
    let (w, h) = (
        crate::fbcon::FbCon::cell_width(),
        crate::fbcon::FbCon::cell_height(),
    );
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

fn timer_ticks_advance() -> Result<(), &'static str> {
    let start = crate::apic::TICKS.load(Ordering::SeqCst);
    // Wait for several ticks; hlt sleeps until the next interrupt.
    for _ in 0..1000 {
        if crate::apic::TICKS.load(Ordering::SeqCst) >= start + 5 {
            return Ok(());
        }
        arch_x86_64::cpu::hlt();
    }
    Err("timer nao avancou 5 ticks")
}

static RAN: AtomicU64 = AtomicU64::new(0);
static PING: AtomicU64 = AtomicU64::new(0);
static SPIN_A: AtomicU64 = AtomicU64::new(0);
static SPIN_B: AtomicU64 = AtomicU64::new(0);

fn mark_ran() {
    RAN.store(1, Ordering::SeqCst);
    loop {
        crate::sched::yield_now();
    }
}

fn spawned_thread_runs() -> Result<(), &'static str> {
    crate::sched::spawn(mark_ran);
    for _ in 0..100 {
        if RAN.load(Ordering::SeqCst) == 1 {
            return Ok(());
        }
        crate::sched::yield_now();
    }
    Err("thread criada nunca executou")
}

fn ping() {
    loop {
        PING.fetch_add(1, Ordering::SeqCst);
        crate::sched::yield_now();
    }
}

fn yield_alternates_threads() -> Result<(), &'static str> {
    crate::sched::spawn(ping);
    let before = PING.load(Ordering::SeqCst);
    for _ in 0..10 {
        crate::sched::yield_now();
    }
    if PING.load(Ordering::SeqCst) > before {
        Ok(())
    } else {
        Err("yield_now nao passou a CPU para a outra thread")
    }
}

fn spin_a() {
    loop {
        SPIN_A.fetch_add(1, Ordering::SeqCst);
    }
}

fn spin_b() {
    loop {
        SPIN_B.fetch_add(1, Ordering::SeqCst);
    }
}

fn preemption_runs_both_threads() -> Result<(), &'static str> {
    crate::sched::spawn(spin_a);
    crate::sched::spawn(spin_b);
    // Neither thread yields; only the timer can make both progress.
    for _ in 0..2000 {
        if SPIN_A.load(Ordering::SeqCst) > 0 && SPIN_B.load(Ordering::SeqCst) > 0 {
            return Ok(());
        }
        arch_x86_64::cpu::hlt();
    }
    Err("o timer nao preemptou as threads que nao cedem a CPU")
}
