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
    ("rsp0_set_before_user", rsp0_set_before_user),
    (
        "enter_user_returns_after_exit",
        enter_user_returns_after_exit,
    ),
    (
        "syscall_roundtrip_preserves_gs",
        syscall_roundtrip_preserves_gs,
    ),
    (
        "syscall_dispatch_records_mark",
        syscall_dispatch_records_mark,
    ),
    ("ring0_exception_keeps_gs", ring0_exception_keeps_gs),
    (
        "kernel_mapped_in_new_address_space",
        kernel_mapped_in_new_address_space,
    ),
    (
        "higher_half_pml4_fully_reserved",
        higher_half_pml4_fully_reserved,
    ),
    (
        "address_space_drop_frees_user_frames",
        address_space_drop_frees_user_frames,
    ),
    ("user_process_runs_in_ring3", user_process_runs_in_ring3),
    // Must stay LAST: the spawned user processes never exit.
    (
        "user_process_exits_and_is_reaped",
        user_process_exits_and_is_reaped,
    ),
    ("pe_loads_runs_and_exits", pe_loads_runs_and_exits),
    ("load_zero_fills_bss", load_zero_fills_bss),
    ("two_user_processes_preempt", two_user_processes_preempt),
    ("reloc_dir64_crosses_page", reloc_dir64_crosses_page),
    ("stack_placed_above_image", stack_placed_above_image),
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

fn ring0_exception_keeps_gs() -> Result<(), &'static str> {
    // A Ring 0 exception (int3) must NOT swapgs: the kernel GS base must be
    // unchanged across it. (The conditional swapgs only fires for RPL 3.)
    let before = arch_x86_64::percpu::gs_base();
    arch_x86_64::cpu::int3();
    let after = arch_x86_64::percpu::gs_base();
    if before != after {
        return Err("excecao em Ring 0 trocou o GS indevidamente");
    }
    Ok(())
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

fn rsp0_set_before_user() -> Result<(), &'static str> {
    // A kernel stack top we control for this check.
    static STACK: [u8; 4096] = [0; 4096];
    let top = (&STACK as *const _ as u64) + 4096;
    unsafe { arch_x86_64::gdt::set_rsp0(top) };
    if arch_x86_64::gdt::rsp0() != top {
        return Err("rsp0 nao refletiu o valor gravado");
    }
    // Per-CPU block must be installed (GS base non-zero) by kmain before tests.
    if arch_x86_64::percpu::gs_base() == 0 {
        return Err("GS base nao instalado");
    }
    Ok(())
}

fn syscall_dispatch_records_mark() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    // Call the dispatch function directly (no privilege change): proves the
    // table wiring and SYS_WRITE_MARK semantics independent of the asm stub.
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    let r = crate::syscall::dispatch(crate::syscall::SYS_WRITE_MARK, 0xCAFE, 0, 0, 0);
    if r != 0 {
        return Err("SYS_WRITE_MARK deveria retornar 0");
    }
    if crate::syscall::LAST_MARK.load(Ordering::SeqCst) != 0xCAFE {
        return Err("SYS_WRITE_MARK nao registrou o argumento");
    }
    Ok(())
}

fn enter_user_returns_after_exit() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    let code = crate::user_demo::run();
    if code != 0x2A {
        return Err("codigo de saida do usuario incorreto");
    }
    if crate::syscall::LAST_MARK.load(Ordering::SeqCst) != 0xCAFE {
        return Err("a syscall de escrita do usuario nao chegou ao kernel");
    }
    Ok(())
}

fn syscall_roundtrip_preserves_gs() -> Result<(), &'static str> {
    // After a full user excursion, the kernel GS base must still point at the
    // per-CPU block (a mismatched swapgs count would leave it wrong).
    let before = arch_x86_64::percpu::gs_base();
    let _ = crate::user_demo::run();
    if arch_x86_64::percpu::gs_base() != before {
        return Err("GS base do kernel mudou apos a excursao (swapgs desbalanceado)");
    }
    Ok(())
}

fn kernel_mapped_in_new_address_space() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let space = crate::mem::space::AddressSpace::new_user();
    // A kernel higher-half address (the heap window) must resolve in the new space
    // to the same physical frame as in the active space.
    let probe = 0xffff_ff80_0000_0000u64; // heap window base (mapped by the heap)
    let active = crate::mem::paging::translate(probe);
    let in_new = crate::mem::paging::translate_in(space.root(), probe);
    if active.is_none() {
        return Err("endereco de prova nao mapeado no espaco ativo");
    }
    if in_new != active {
        return Err("metade alta do kernel nao foi clonada no novo espaco");
    }
    // A low (user) address must be unmapped in the fresh space.
    if crate::mem::paging::translate_in(space.root(), 0x40_0000).is_some() {
        return Err("metade baixa deveria estar vazia");
    }
    let _ = PageFlags::USER;
    Ok(())
}

fn address_space_drop_frees_user_frames() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let before = crate::mem::frames::free_frame_count();
    {
        let space = crate::mem::space::AddressSpace::new_user();
        let frame = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
        unsafe {
            space
                .map(0x40_0000, frame, PageFlags::USER.union(PageFlags::WRITABLE))
                .map_err(|_| "map no espaco novo falhou")?;
        }
        // Dropping `space` must free: the mapped user frame, the intermediate
        // tables, and the PML4 — leaving the count where it started.
    }
    let after = crate::mem::frames::free_frame_count();
    if after != before {
        return Err("Drop do AddressSpace nao liberou todos os frames de usuario");
    }
    Ok(())
}

fn user_process_runs_in_ring3() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    // Payload: SYS_WRITE_MARK(0xBEEF) in a loop.
    crate::process::spawn_demo_user();
    // The boot thread yields; the user process must run in Ring 3 and reach the kernel.
    for _ in 0..2000 {
        if crate::syscall::LAST_MARK.load(Ordering::SeqCst) == 0xBEEF {
            return Ok(());
        }
        crate::sched::yield_now();
    }
    Err("o processo de usuario nao executou em Ring 3")
}

static USER_A_RAN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static USER_B_RAN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Called from the syscall dispatcher for every SYS_WRITE_MARK.
pub fn on_user_mark(mark: u64) {
    use core::sync::atomic::Ordering;
    match mark {
        0xA => {
            USER_A_RAN.fetch_add(1, Ordering::SeqCst);
        }
        0xB => {
            USER_B_RAN.fetch_add(1, Ordering::SeqCst);
        }
        _ => {}
    }
}

fn user_process_exits_and_is_reaped() -> Result<(), &'static str> {
    let before = crate::mem::frames::free_frame_count();
    // Spawn a user process whose payload does SYS_EXIT immediately.
    crate::process::spawn_exiting_user();
    // Yield until it has run, exited and been reaped (its space+stacks freed).
    for _ in 0..4000 {
        crate::sched::yield_now();
        if crate::mem::frames::free_frame_count() == before {
            return Ok(());
        }
    }
    Err("processo de usuario nao foi recolhido (frames nao voltaram)")
}

fn pe_loads_runs_and_exits() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    let before = crate::mem::frames::free_frame_count();
    crate::syscall::LAST_MARK.store(0, Ordering::SeqCst);
    // The embedded test PE: SYS_WRITE_MARK(0x5E) then SYS_EXIT(0).
    crate::process::spawn_test_pe();
    let mut marked = false;
    for _ in 0..8000 {
        crate::sched::yield_now();
        if crate::syscall::LAST_MARK.load(Ordering::SeqCst) == 0x5E {
            marked = true;
        }
        if marked && crate::mem::frames::free_frame_count() == before {
            return Ok(());
        }
    }
    Err("PE nao carregou/rodou/saiu com recuperacao de frames")
}

fn load_zero_fills_bss() -> Result<(), &'static str> {
    // A section with virtual_size > raw.len() must be zero-filled by the loader.
    use crate::mem::space::AddressSpace;
    let space = AddressSpace::new_user();
    // Build a 1-page section at RVA 0x1000, raw = [0xAA], virtual_size = 0x20.
    let image = crate::process::build_bss_test_pe();
    let base = 0x40_0000u64;
    let entry = crate::loader::load_pe(&space, &image, base).map_err(|_| "load falhou")?;
    let _ = entry;
    // Read the mapped page via the space: byte 0 == 0xAA, byte 1..0x20 == 0.
    let phys = space.translate(base + 0x1000).ok_or("secao nao mapeada")?;
    let p = crate::mem::frames::phys_to_virt(phys) as *const u8;
    unsafe {
        if core::ptr::read_volatile(p) != 0xAA {
            return Err("byte bruto nao copiado");
        }
        if core::ptr::read_volatile(p.add(1)) != 0x00 {
            return Err("excedente do .bss nao foi zerado");
        }
    }
    Ok(())
}

fn two_user_processes_preempt() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;
    USER_A_RAN.store(0, Ordering::SeqCst);
    USER_B_RAN.store(0, Ordering::SeqCst);
    // Two user processes that never yield; only the timer makes both run.
    crate::process::spawn_demo_user_pair();
    for _ in 0..4000 {
        if USER_A_RAN.load(Ordering::SeqCst) > 0 && USER_B_RAN.load(Ordering::SeqCst) > 0 {
            return Ok(());
        }
        arch_x86_64::cpu::hlt();
    }
    Err("o timer nao preemptou os dois processos de usuario")
}

fn higher_half_pml4_fully_reserved() -> Result<(), &'static str> {
    use mm::paging::entry_present;
    // Every PML4 entry 256..512 of the active space must be present after init,
    // so a new address space clones a complete kernel higher half.
    let pml4 = crate::arch_cr3_base();
    let table = crate::mem::frames::phys_to_virt(pml4) as *const u64;
    for i in 256..512 {
        let e = unsafe { core::ptr::read_volatile(table.add(i)) };
        if !entry_present(e) {
            return Err("entrada PML4 da metade alta nao reservada");
        }
    }
    Ok(())
}

fn reloc_dir64_crosses_page() -> Result<(), &'static str> {
    use crate::mem::space::AddressSpace;
    // PE with a DIR64 relocation whose 8-byte target straddles a page boundary
    // (target RVA 0x1FFC), loaded at a base != image_base so delta != 0.
    let image = crate::process::build_reloc_cross_page_pe();
    let space = AddressSpace::new_user();
    let load_base = 0x80_0000u64;
    crate::loader::load_pe(&space, &image, load_base).map_err(|_| "load falhou")?;
    let lo_phys = space
        .translate(load_base + 0x1000)
        .ok_or("pagina 1 nao mapeada")?;
    let hi_phys = space
        .translate(load_base + 0x2000)
        .ok_or("pagina 2 nao mapeada")?;
    let lo = crate::mem::frames::phys_to_virt(lo_phys) as *const u8;
    let hi = crate::mem::frames::phys_to_virt(hi_phys) as *const u8;
    let mut bytes = [0u8; 8];
    unsafe {
        core::ptr::copy_nonoverlapping(lo.add(0xFFC), bytes.as_mut_ptr(), 4);
        core::ptr::copy_nonoverlapping(hi, bytes.as_mut_ptr().add(4), 4);
    }
    let got = u64::from_le_bytes(bytes);
    let expected = crate::process::RELOC_FIXTURE_ORIGINAL
        .wrapping_add(load_base.wrapping_sub(crate::process::RELOC_FIXTURE_IMAGE_BASE));
    if got != expected {
        return Err("relocacao entre paginas incorreta");
    }
    Ok(())
}

fn stack_placed_above_image() -> Result<(), &'static str> {
    // image_top of the standard test PE must be above its last section end.
    let image = crate::process::test_pe_bytes();
    let base = 0x40_0000u64;
    let top = crate::loader::image_top(image, base).map_err(|_| "image_top falhou")?;
    if top <= base + 0x1000 {
        return Err("image_top nao ficou acima da imagem");
    }
    Ok(())
}
