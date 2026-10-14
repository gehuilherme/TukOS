# TukOS Plano 2 — Fundação de memória: frames físicos, paginação e heap

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** O kernel do TukOS gerencia memória: um alocador de frames físicos construído a partir do mapa do Limine, um mapeador de páginas de 4 níveis que cria e remove mapeamentos no espaço de endereços ativo, e um heap de kernel que habilita `alloc` (`Box`, `Vec`). Mais os carryovers baratos de diagnóstico da revisão do Plano 1 (panic na tela + backtrace).

**Architecture:** A lógica pura vive na nova crate `crates/mm` (bitmap de frames, aritmética de endereço virtual e de entradas de tabela de página, free-list do heap), toda testável no host com `cargo xtask test`. O kernel integra essa lógica com o hardware real usando o HHDM (higher-half direct map) e o mapa de memória que o Limine fornece, e expõe cada capacidade por um autoteste do kernel (`cargo xtask ktest`). Nenhuma interrupção é habilitada neste plano, então os locks continuam `spin::Mutex`.

**Tech Stack:** Rust 1.97.1 stable (edition 2024, sem nightly), crate `limine` 0.5.0 (`HhdmRequest`, `MemoryMapRequest`), `spin` 0.10, QEMU + OVMF. Sem novas dependências externas: o alocador de frames, o mapeador e o heap são escritos do zero (também por higiene de clean room).

**Spec:** `docs/superpowers/specs/2026-10-13-tukos-marco1-design.md` (seção 4, itens de memória: alocador de páginas físicas, paginação de 4 níveis com espaço por processo, heap do kernel). Este é o **Plano 2 de 4** do Marco 1 e sucede o Plano 1 (fundação), já na branch `main`.

**Escopo deliberadamente fora deste plano (vai para o Plano 2b):** scheduler e troca de contexto, timer do LAPIC e habilitar interrupções, modo usuário via SYSCALL/SYSRET, locks seguros para interrupção (save/restore RFLAGS), `swapgs` + base de GS por CPU + `gdt::set_rsp0`, IST própria para NMI/#MC, teste de double fault por estouro de pilha, e o alvo `cargo xtask vmware`. Estão registrados na memória do projeto e são os primeiros itens do Plano 2b.

## Global Constraints

- Toolchain fixada: Rust `1.97.1` stable via `rust-toolchain.toml`. Proibido usar recursos nightly.
- Edition `2024` em todas as crates. Licença `MIT OR Apache-2.0` em todos os `Cargo.toml`.
- Apenas x86-64. Código dependente de CPU vive em `crates/arch-x86_64`; lógica pura de memória vive em `crates/mm`.
- Nome do projeto: TukOS. Proibido usar "Windows", "Win" ou identidade visual da Microsoft em nomes, marcas ou textos de interface.
- Política de clean room (spec seção 2): proibido código vazado da Microsoft; este plano usa apenas fontes de nível 0 (Intel SDM Vol. 3A capítulo 4 "Paging"; especificação do protocolo Limine; AMD64 APM para a semântica de paginação).
- Todo commit que implementa comportamento de hardware/protocolo declara a fonte no rodapé: `Source: docs <documento>`.
- Commits terminam com um rodapé `Co-Authored-By:` nomeando o modelo que de fato escreveu o commit.
- Identificadores e comentários de código em inglês; documentação do repositório em português.
- Tamanho de página: 4096 bytes (4 KiB). Endereços físicos e virtuais são `u64`. O kernel está ligado em `0xffffffff80000000`.
- Marcadores de serial já existentes (contrato com o `xtask`, não mudam): `@@TUKOS-KTEST-PASS <nome>`, `@@TUKOS-KTEST-FAIL <nome> <motivo>`, `@@TUKOS-KTEST-DONE passed=<n> failed=<n>`, `@@TUKOS-PANIC <mensagem>`.
- O QEMU abre janela por padrão; a CI e `TUKOS_HEADLESS=1` rodam sem janela (comportamento do Plano 1, não mudar).

## Review Focus

1. **Frame devolvido pelo alocador que se sobrepõe ao próprio bitmap, ao kernel ou a região não-usável** corromperia dados vivos; o alocador só pode entregar frames marcados usáveis e nunca os que guardam seu próprio bitmap. Coberto por `alloc_only_returns_usable_frames` e pela exclusão do bitmap no ktest `frame_alloc_roundtrip` (Tasks 1 e 2).
2. **Mapa de memória com regiões não ordenadas, adjacentes ou de tamanho não múltiplo de página** (o Limine não garante ordenação) faria o cálculo de total de frames ou de offset do bitmap errar; a construção deve alinhar e tolerar qualquer ordem. Coberto por `total_frames_uses_highest_region_end` e `mark_region_handles_unaligned_bounds` (Task 1).
3. **Mapear um endereço virtual cuja tabela intermediária ainda não existe** deve criar a tabela faltante (alocando um frame e zerando-o), não escrever num ponteiro inválido. Coberto pelo ktest `map_creates_missing_tables` (Task 4).
4. **`alloc`/`dealloc` do heap com alinhamento maior que o padrão, ou que esgota a região** deve respeitar o `Layout` e devolver nulo ao esgotar, nunca entregar memória desalinhada ou sobreposta. Coberto por `heap_respects_alignment`, `heap_returns_null_when_exhausted` e `heap_reuses_freed_block` (Task 5).
5. **Dupla liberação ou liberação de frame fora de faixa** no alocador de frames deve ser rejeitada/ignorada de forma segura, não decrementar o contador duas vezes nem escrever fora do bitmap. Coberto por `double_free_is_idempotent` e `free_out_of_range_is_ignored` (Task 1).

---

### Task 1: Crate `mm` — alocador de frames por bitmap (lógica pura)

**Files:**
- Create: `crates/mm/Cargo.toml`, `crates/mm/src/lib.rs`, `crates/mm/src/frame.rs`
- Modify: `Cargo.toml` (workspace: adicionar `crates/mm` a `members` e `default-members`)

**Interfaces:**
- Produces (em `crates/mm/src/frame.rs`):
  - `pub const FRAME_SIZE: u64 = 4096;`
  - `pub struct BitmapAllocator<'a>` com `&'a mut [u64]` de armazenamento
  - `pub fn new(bitmap: &'a mut [u64], total_frames: usize) -> Self` — todos os frames começam marcados como usados
  - `pub fn mark_region_usable(&mut self, base: u64, length: u64)` — marca livres os frames inteiramente contidos em `[base, base+length)` após alinhar para dentro
  - `pub fn mark_frame_used(&mut self, frame: usize)`
  - `pub fn alloc(&mut self) -> Option<usize>` — índice do frame; marca usado
  - `pub fn free(&mut self, frame: usize)` — ignora índice fora de faixa ou já livre
  - `pub fn free_count(&self) -> usize`
  - `pub fn total_frames(&self) -> usize`
  - `pub const fn bitmap_words_for(total_frames: usize) -> usize` — número de `u64` necessários
  - `pub const fn total_frames_for(highest_addr: u64) -> usize` — `ceil(highest_addr / FRAME_SIZE)`

- [ ] **Step 1: Criar a crate e registrá-la no workspace**

Em `Cargo.toml` (raiz):
```toml
members = ["xtask", "crates/arch-x86_64", "crates/mm", "kernel"]
default-members = ["xtask", "crates/arch-x86_64", "crates/mm"]
```

`crates/mm/Cargo.toml`:
```toml
[package]
name = "mm"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false
```

`crates/mm/src/lib.rs`:
```rust
//! Pure, host-testable memory-management logic: physical frame allocation,
//! virtual-address and page-table-entry arithmetic, and the kernel heap free list.
//! No hardware access lives here — the kernel wires this to real memory.
#![cfg_attr(not(test), no_std)]

pub mod frame;
```

- [ ] **Step 2: Escrever os testes que falham**

`crates/mm/src/frame.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn allocator(total_frames: usize) -> (Vec<u64>, usize) {
        (vec![0u64; bitmap_words_for(total_frames)], total_frames)
    }

    #[test]
    fn total_frames_for_rounds_up() {
        assert_eq!(total_frames_for(0), 0);
        assert_eq!(total_frames_for(1), 1);
        assert_eq!(total_frames_for(FRAME_SIZE), 1);
        assert_eq!(total_frames_for(FRAME_SIZE + 1), 2);
    }

    #[test]
    fn bitmap_words_for_rounds_up_to_u64() {
        assert_eq!(bitmap_words_for(0), 0);
        assert_eq!(bitmap_words_for(1), 1);
        assert_eq!(bitmap_words_for(64), 1);
        assert_eq!(bitmap_words_for(65), 2);
    }

    #[test]
    fn new_starts_fully_used() {
        let (mut words, total) = allocator(128);
        let a = BitmapAllocator::new(&mut words, total);
        assert_eq!(a.free_count(), 0);
        assert_eq!(a.total_frames(), 128);
    }

    #[test]
    fn mark_region_frees_contained_frames_and_alloc_roundtrips() {
        let (mut words, total) = allocator(16);
        let mut a = BitmapAllocator::new(&mut words, total);
        // Frames 2,3,4 fully inside [2*4096, 5*4096).
        a.mark_region_usable(2 * FRAME_SIZE, 3 * FRAME_SIZE);
        assert_eq!(a.free_count(), 3);
        let f = a.alloc().unwrap();
        assert!((2..=4).contains(&f));
        assert_eq!(a.free_count(), 2);
        a.free(f);
        assert_eq!(a.free_count(), 3);
    }

    #[test]
    fn mark_region_handles_unaligned_bounds() {
        let (mut words, total) = allocator(16);
        let mut a = BitmapAllocator::new(&mut words, total);
        // [4096+1, 4096+1 + 3*4096): only frames 2 and 3 are fully contained.
        a.mark_region_usable(FRAME_SIZE + 1, 3 * FRAME_SIZE);
        assert_eq!(a.free_count(), 2);
    }

    #[test]
    fn alloc_only_returns_usable_frames() {
        let (mut words, total) = allocator(64);
        let mut a = BitmapAllocator::new(&mut words, total);
        a.mark_region_usable(10 * FRAME_SIZE, 2 * FRAME_SIZE); // frames 10,11
        let mut seen = alloc::vec::Vec::new();
        while let Some(f) = a.alloc() {
            assert!(f == 10 || f == 11);
            assert!(!seen.contains(&f), "frame devolvido duas vezes");
            seen.push(f);
        }
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn mark_frame_used_removes_from_free_pool() {
        let (mut words, total) = allocator(16);
        let mut a = BitmapAllocator::new(&mut words, total);
        a.mark_region_usable(0, 16 * FRAME_SIZE);
        a.mark_frame_used(5);
        assert_eq!(a.free_count(), 15);
        for _ in 0..15 {
            assert_ne!(a.alloc(), Some(5));
        }
        assert_eq!(a.alloc(), None);
    }

    #[test]
    fn double_free_is_idempotent() {
        let (mut words, total) = allocator(16);
        let mut a = BitmapAllocator::new(&mut words, total);
        a.mark_region_usable(0, 4 * FRAME_SIZE);
        let f = a.alloc().unwrap();
        a.free(f);
        a.free(f);
        assert_eq!(a.free_count(), 4);
    }

    #[test]
    fn free_out_of_range_is_ignored() {
        let (mut words, total) = allocator(16);
        let mut a = BitmapAllocator::new(&mut words, total);
        a.mark_region_usable(0, 4 * FRAME_SIZE);
        a.free(9999);
        assert_eq!(a.free_count(), 4);
    }

    #[test]
    fn total_frames_uses_highest_region_end() {
        // A gap then a high region: total frames must cover up to the highest end.
        let total = total_frames_for(100 * FRAME_SIZE);
        let (mut words, _) = allocator(total);
        let mut a = BitmapAllocator::new(&mut words, total);
        a.mark_region_usable(0, FRAME_SIZE);
        a.mark_region_usable(99 * FRAME_SIZE, FRAME_SIZE);
        assert_eq!(a.free_count(), 2);
        assert_eq!(a.total_frames(), 100);
    }
}
```

O módulo de testes usa `alloc::vec::Vec` e `vec!`; no topo do arquivo adicione, **apenas para teste**, `#[cfg(test)] extern crate alloc;` logo abaixo do código de produção, e no host `Vec`/`vec!` do std já estão disponíveis — para simplificar, troque `alloc::vec::Vec` por `Vec` e mantenha `vec!` (os testes rodam com std). Ou seja: use `Vec` e `vec!` diretamente.

- [ ] **Step 3: Rodar e ver falhar**

Run: `cargo test -p mm`
Expected: FAIL de compilação — `cannot find function total_frames_for`, `cannot find type BitmapAllocator`, etc.

- [ ] **Step 4: Implementar**

Acima do bloco de testes em `crates/mm/src/frame.rs`:
```rust
//! Bitmap physical frame allocator. One bit per 4 KiB frame; 1 = used, 0 = free.
//! Storage is caller-provided so the kernel can place it in a real memory region.

/// Size of a physical frame, in bytes.
pub const FRAME_SIZE: u64 = 4096;

/// Number of frames needed to cover addresses up to (but not including) `highest_addr`.
pub const fn total_frames_for(highest_addr: u64) -> usize {
    highest_addr.div_ceil(FRAME_SIZE) as usize
}

/// Number of `u64` words needed to hold one bit per frame.
pub const fn bitmap_words_for(total_frames: usize) -> usize {
    total_frames.div_ceil(64)
}

pub struct BitmapAllocator<'a> {
    bitmap: &'a mut [u64],
    total_frames: usize,
    free: usize,
}

impl<'a> BitmapAllocator<'a> {
    /// Creates an allocator with every frame marked used. `bitmap` must hold at
    /// least `bitmap_words_for(total_frames)` words.
    pub fn new(bitmap: &'a mut [u64], total_frames: usize) -> Self {
        for word in bitmap.iter_mut() {
            *word = u64::MAX;
        }
        Self {
            bitmap,
            total_frames,
            free: 0,
        }
    }

    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    pub fn free_count(&self) -> usize {
        self.free
    }

    fn is_used(&self, frame: usize) -> bool {
        self.bitmap[frame / 64] & (1u64 << (frame % 64)) != 0
    }

    fn set_used(&mut self, frame: usize) {
        self.bitmap[frame / 64] |= 1u64 << (frame % 64);
    }

    fn set_free_bit(&mut self, frame: usize) {
        self.bitmap[frame / 64] &= !(1u64 << (frame % 64));
    }

    /// Marks every frame fully contained in `[base, base + length)` as free.
    pub fn mark_region_usable(&mut self, base: u64, length: u64) {
        if length == 0 {
            return;
        }
        let first = base.div_ceil(FRAME_SIZE);
        let end = (base + length) / FRAME_SIZE; // exclusive; floor keeps only full frames
        let mut frame = first as usize;
        let last = end as usize;
        while frame < last && frame < self.total_frames {
            if self.is_used(frame) {
                self.set_free_bit(frame);
                self.free += 1;
            }
            frame += 1;
        }
    }

    pub fn mark_frame_used(&mut self, frame: usize) {
        if frame < self.total_frames && !self.is_used(frame) {
            self.set_used(frame);
            self.free -= 1;
        }
    }

    pub fn alloc(&mut self) -> Option<usize> {
        if self.free == 0 {
            return None;
        }
        for (word_index, word) in self.bitmap.iter().enumerate() {
            if *word != u64::MAX {
                let bit = word.trailing_ones() as usize;
                let frame = word_index * 64 + bit;
                if frame >= self.total_frames {
                    break;
                }
                self.set_used(frame);
                self.free -= 1;
                return Some(frame);
            }
        }
        None
    }

    pub fn free(&mut self, frame: usize) {
        if frame < self.total_frames && self.is_used(frame) {
            self.set_free_bit(frame);
            self.free += 1;
        }
    }
}
```

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo test -p mm`
Expected: PASS em todos os testes de `frame`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/mm
git commit -m "feat(mm): alocador de frames físicos por bitmap (lógica pura)

Source: docs Intel SDM Vol. 3A, seção 4 (Paging), conceito de frame físico
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 2: Kernel — construir o alocador de frames a partir do mapa do Limine

**Files:**
- Create: `kernel/src/mem/mod.rs`, `kernel/src/mem/frames.rs`
- Modify: `kernel/Cargo.toml` (dependência `mm`), `kernel/src/main.rs` (requests + `mod mem;` + init), `kernel/src/ktest.rs` (novo teste)

**Interfaces:**
- Consumes: `mm::frame::{BitmapAllocator, FRAME_SIZE, total_frames_for, bitmap_words_for}` (Task 1); `limine::request::{HhdmRequest, MemoryMapRequest}`, `limine::memory_map::EntryType`.
- Produces:
  - `kernel/src/mem/frames.rs`: `pub fn init(hhdm_offset: u64, map: &[&limine::memory_map::Entry])`; `pub fn alloc_frame() -> Option<u64>` (endereço físico); `pub fn free_frame(phys: u64)`; `pub fn free_frame_count() -> usize`; `pub fn hhdm_offset() -> u64`; `pub fn phys_to_virt(phys: u64) -> u64` (= `phys + hhdm_offset`).
  - `kernel/src/mem/mod.rs`: `pub mod frames;` e `pub fn init()` que lê as respostas do Limine e chama `frames::init`.

- [ ] **Step 1: Escrever o autoteste do kernel que falha**

Em `kernel/src/ktest.rs`, adicionar `("frame_alloc_roundtrip", frame_alloc_roundtrip),` ao fim de `TESTS` e a função:
```rust
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
```

Reexporte `FRAME_SIZE` em `frames.rs` como `pub use mm::frame::FRAME_SIZE;` para o teste acima.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest`
Expected: FAIL de compilação — `crate::mem` não existe.

- [ ] **Step 3: Implementar**

Em `kernel/Cargo.toml`, em `[dependencies]`: `mm = { path = "../crates/mm" }`.

`kernel/src/mem/mod.rs`:
```rust
//! Kernel memory management: physical frames, paging and the heap.

pub mod frames;

/// Reads the Limine HHDM and memory-map responses and brings up the frame allocator.
pub fn init() {
    let hhdm = crate::HHDM_REQUEST
        .get_response()
        .expect("Limine nao forneceu HHDM")
        .offset();
    let map = crate::MEMORY_MAP_REQUEST
        .get_response()
        .expect("Limine nao forneceu o mapa de memoria");
    frames::init(hhdm, map.entries());
}
```

`kernel/src/mem/frames.rs`:
```rust
//! Global physical frame allocator, backed by `mm::frame::BitmapAllocator`.
//! The bitmap itself is carved out of the largest usable region and addressed
//! through the HHDM, so no chicken-and-egg allocation is needed.

use limine::memory_map::{Entry, EntryType};
use mm::frame::{BitmapAllocator, bitmap_words_for, total_frames_for};
use spin::Mutex;

pub use mm::frame::FRAME_SIZE;

struct State {
    allocator: BitmapAllocator<'static>,
    hhdm: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

pub fn hhdm_offset() -> u64 {
    STATE.lock().as_ref().expect("frames nao inicializado").hhdm
}

pub fn phys_to_virt(phys: u64) -> u64 {
    phys + hhdm_offset()
}

pub fn free_frame_count() -> usize {
    STATE
        .lock()
        .as_ref()
        .map(|s| s.allocator.free_count())
        .unwrap_or(0)
}

pub fn alloc_frame() -> Option<u64> {
    let mut guard = STATE.lock();
    let state = guard.as_mut()?;
    state.allocator.alloc().map(|f| f as u64 * FRAME_SIZE)
}

pub fn free_frame(phys: u64) {
    if let Some(state) = STATE.lock().as_mut() {
        state.allocator.free((phys / FRAME_SIZE) as usize);
    }
}

/// Builds the allocator from the Limine memory map.
///
/// Strategy: find the highest region end to size the bitmap, place the bitmap at
/// the start of the largest usable region (reached via the HHDM), then mark every
/// usable frame free and immediately reserve the frames the bitmap occupies.
pub fn init(hhdm: u64, map: &[&Entry]) {
    let highest_end = map.iter().map(|e| e.base + e.length).max().unwrap_or(0);
    let total_frames = total_frames_for(highest_end);
    let words = bitmap_words_for(total_frames);
    let bitmap_bytes = (words * core::mem::size_of::<u64>()) as u64;

    // Largest usable region holds the bitmap.
    let host = map
        .iter()
        .filter(|e| e.entry_type == EntryType::USABLE)
        .max_by_key(|e| e.length)
        .expect("nenhuma regiao usavel no mapa de memoria");
    assert!(
        host.length >= bitmap_bytes,
        "maior regiao usavel nao comporta o bitmap de frames"
    );

    let bitmap_phys = host.base;
    let bitmap_ptr = (bitmap_phys + hhdm) as *mut u64;
    let bitmap: &'static mut [u64] =
        unsafe { core::slice::from_raw_parts_mut(bitmap_ptr, words) };

    let mut allocator = BitmapAllocator::new(bitmap, total_frames);
    for e in map.iter().filter(|e| e.entry_type == EntryType::USABLE) {
        allocator.mark_region_usable(e.base, e.length);
    }

    // Reserve the frames the bitmap itself occupies.
    let bitmap_frames = bitmap_bytes.div_ceil(FRAME_SIZE);
    let first = bitmap_phys / FRAME_SIZE;
    for f in first..first + bitmap_frames {
        allocator.mark_frame_used(f as usize);
    }

    *STATE.lock() = Some(State { allocator, hhdm });
}
```

Em `kernel/src/main.rs`:
- adicionar `mod mem;` junto aos demais módulos;
- trocar o `use limine::request::...` por `use limine::request::{FramebufferRequest, HhdmRequest, MemoryMapRequest, RequestsEndMarker, RequestsStartMarker};`;
- acrescentar, junto aos outros requests em `.requests`:
```rust
#[used]
#[unsafe(link_section = ".requests")]
pub static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();
```
- em `kmain`, após o `assert!(BASE_REVISION...)` e antes de `kprintln!("TukOS pronto")`, chamar:
```rust
    mem::init();
    kprintln!(
        "memoria: {} frames livres",
        mem::frames::free_frame_count()
    );
```

Se os nomes da crate `limine` 0.5 divergirem (`get_response`, `offset`, `entries`, `entry_type`, `EntryType::USABLE`, `base`, `length`), siga `~/.cargo/registry/src/*/limine-0.5.0/src`; o comportamento exigido não muda.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo xtask ktest`
Expected: `variante ktest ok (7 teste(s))` e as variantes de panic/fault ok.

Run: `cargo xtask run` (headless opcional com `TUKOS_HEADLESS=1`)
Expected: a serial mostra `memoria: N frames livres` com N > 0.

- [ ] **Step 5: Commit**

```bash
git add kernel
git commit -m "feat(kernel): alocador global de frames a partir do mapa do Limine

Source: docs Limine boot protocol (memory map, HHDM)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 3: Crate `mm` — endereço virtual e entrada de tabela de página (lógica pura)

**Files:**
- Create: `crates/mm/src/paging.rs`
- Modify: `crates/mm/src/lib.rs` (adicionar `pub mod paging;`)

**Interfaces:**
- Produces (em `crates/mm/src/paging.rs`):
  - `pub const PAGE_SIZE: u64 = 4096;`
  - `pub fn page_table_indices(virt: u64) -> [usize; 4]` — índices `[p4, p3, p2, p1]` (bits 47..39, 38..30, 29..21, 20..12)
  - `pub fn page_offset(virt: u64) -> u64` (bits 11..0)
  - `pub struct PageFlags(u64)` com `pub const PRESENT/WRITABLE/USER/HUGE/NO_EXECUTE: PageFlags`, `pub const fn empty()`, `pub const fn union(self, other: PageFlags)`, `pub const fn contains(self, other: PageFlags) -> bool`, `pub const fn bits(self) -> u64`
  - `pub fn make_entry(frame_phys: u64, flags: PageFlags) -> u64` — combina o endereço físico (alinhado) com as flags
  - `pub fn entry_frame(entry: u64) -> u64` — extrai o endereço físico (bits 51..12, zera flags)
  - `pub fn entry_flags(entry: u64) -> PageFlags`
  - `pub fn entry_present(entry: u64) -> bool`

- [ ] **Step 1: Escrever os testes que falham**

`crates/mm/src/paging.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_split_canonical_higher_half() {
        // Kernel base 0xffffffff80000000.
        let idx = page_table_indices(0xffff_ffff_8000_0000);
        assert_eq!(idx, [511, 510, 0, 0]);
        assert_eq!(page_offset(0xffff_ffff_8000_0abc), 0xabc);
    }

    #[test]
    fn indices_split_low_address() {
        assert_eq!(page_table_indices(0x0000_0000_0000_0000), [0, 0, 0, 0]);
        assert_eq!(page_table_indices(0x0000_0000_0020_1000), [0, 0, 1, 1]);
    }

    #[test]
    fn make_and_read_entry_roundtrip() {
        let frame = 0x1234_5000u64;
        let flags = PageFlags::PRESENT.union(PageFlags::WRITABLE);
        let e = make_entry(frame, flags);
        assert_eq!(entry_frame(e), frame);
        assert!(entry_present(e));
        assert!(entry_flags(e).contains(PageFlags::WRITABLE));
        assert!(!entry_flags(e).contains(PageFlags::USER));
    }

    #[test]
    fn no_execute_is_bit_63() {
        let e = make_entry(0x1000, PageFlags::PRESENT.union(PageFlags::NO_EXECUTE));
        assert!(e & (1 << 63) != 0);
        assert!(entry_flags(e).contains(PageFlags::NO_EXECUTE));
    }

    #[test]
    fn entry_frame_masks_flags_and_nx() {
        // Present+writable+NX on frame 0x2000: extracting the frame drops all of them.
        let e = make_entry(0x2000, PageFlags::PRESENT.union(PageFlags::WRITABLE).union(PageFlags::NO_EXECUTE));
        assert_eq!(entry_frame(e), 0x2000);
    }

    #[test]
    fn not_present_entry_reads_absent() {
        assert!(!entry_present(0));
        assert!(!entry_present(make_entry(0x1000, PageFlags::WRITABLE)));
    }
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p mm paging`
Expected: FAIL de compilação — `cannot find function page_table_indices`.

- [ ] **Step 3: Implementar**

Acima dos testes em `crates/mm/src/paging.rs`:
```rust
//! Virtual-address decomposition and x86-64 page-table entry encoding.
//! Intel SDM Vol. 3A, section 4.5 (4-Level Paging).

pub const PAGE_SIZE: u64 = 4096;

const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000; // bits 51..12

/// `[p4, p3, p2, p1]` table indices for a virtual address.
pub fn page_table_indices(virt: u64) -> [usize; 4] {
    [
        ((virt >> 39) & 0x1FF) as usize,
        ((virt >> 30) & 0x1FF) as usize,
        ((virt >> 21) & 0x1FF) as usize,
        ((virt >> 12) & 0x1FF) as usize,
    ]
}

pub fn page_offset(virt: u64) -> u64 {
    virt & 0xFFF
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PageFlags(u64);

impl PageFlags {
    pub const PRESENT: PageFlags = PageFlags(1 << 0);
    pub const WRITABLE: PageFlags = PageFlags(1 << 1);
    pub const USER: PageFlags = PageFlags(1 << 2);
    pub const HUGE: PageFlags = PageFlags(1 << 7);
    pub const NO_EXECUTE: PageFlags = PageFlags(1 << 63);

    pub const fn empty() -> PageFlags {
        PageFlags(0)
    }

    pub const fn union(self, other: PageFlags) -> PageFlags {
        PageFlags(self.0 | other.0)
    }

    pub const fn contains(self, other: PageFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn bits(self) -> u64 {
        self.0
    }
}

pub fn make_entry(frame_phys: u64, flags: PageFlags) -> u64 {
    (frame_phys & ADDR_MASK) | flags.bits()
}

pub fn entry_frame(entry: u64) -> u64 {
    entry & ADDR_MASK
}

pub fn entry_flags(entry: u64) -> PageFlags {
    PageFlags(entry & !ADDR_MASK)
}

pub fn entry_present(entry: u64) -> bool {
    entry & PageFlags::PRESENT.bits() != 0
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test -p mm`
Expected: PASS em `frame` e `paging`.

- [ ] **Step 5: Commit**

```bash
git add crates/mm
git commit -m "feat(mm): aritmetica de endereco virtual e entradas de tabela de pagina

Source: docs Intel SDM Vol. 3A, secao 4.5 (4-Level Paging)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 4: Kernel — mapeador de páginas sobre as tabelas ativas

**Files:**
- Modify: `crates/arch-x86_64/src/cpu.rs` (adicionar `read_cr3`, `write_cr3`, `invlpg`)
- Create: `kernel/src/mem/paging.rs`
- Modify: `kernel/src/mem/mod.rs` (`pub mod paging;`), `kernel/src/ktest.rs` (novos testes)

**Interfaces:**
- Consumes: `mm::paging::{page_table_indices, PageFlags, make_entry, entry_frame, entry_present, PAGE_SIZE}` (Task 3); `frames::{alloc_frame, phys_to_virt, hhdm_offset}` (Task 2).
- Produces:
  - `arch_x86_64::cpu::read_cr3() -> u64`; `unsafe fn write_cr3(phys: u64)`; `unsafe fn invlpg(virt: u64)`
  - `kernel/src/mem/paging.rs`: `pub unsafe fn map_page(virt: u64, phys: u64, flags: PageFlags) -> Result<(), MapError>`; `pub unsafe fn unmap_page(virt: u64) -> Result<(), MapError>`; `pub fn translate(virt: u64) -> Option<u64>` (retorna o físico mapeado); `pub enum MapError { AllocFailed, AlreadyMapped, NotMapped, HugePage }`.

- [ ] **Step 1: Escrever os autotestes do kernel que falham**

Em `kernel/src/ktest.rs`, adicionar ao fim de `TESTS`:
```rust
    ("map_translate_unmap", map_translate_unmap),
    ("map_creates_missing_tables", map_creates_missing_tables),
```
e as funções:
```rust
fn map_translate_unmap() -> Result<(), &'static str> {
    use mm::paging::PageFlags;
    let phys = crate::mem::frames::alloc_frame().ok_or("sem frame")?;
    // A free slot in the non-canonical-safe test window well above the kernel heap.
    let virt = 0xffff_f000_0000_0000u64;
    let flags = PageFlags::PRESENT.union(PageFlags::WRITABLE).union(PageFlags::NO_EXECUTE);
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
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo xtask ktest`
Expected: FAIL de compilação — `crate::mem::paging` não existe.

- [ ] **Step 3: Implementar as primitivas de CPU**

Em `crates/arch-x86_64/src/cpu.rs`, acrescentar:
```rust
#[inline]
pub fn read_cr3() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// # Safety
/// `phys` must point at a valid PML4 for the current address space.
#[inline]
pub unsafe fn write_cr3(phys: u64) {
    unsafe { asm!("mov cr3, {}", in(reg) phys, options(nostack, preserves_flags)) };
}

/// # Safety
/// Invalidates the TLB entry for `virt`; always safe to call, `unsafe` only for symmetry.
#[inline]
pub unsafe fn invlpg(virt: u64) {
    unsafe { asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags)) };
}
```

- [ ] **Step 4: Implementar o mapeador**

`kernel/src/mem/paging.rs`:
```rust
//! 4-level page mapping over the currently active address space (the one Limine
//! set up). Intermediate tables are reached through the HHDM and allocated from
//! the global frame allocator when missing.

use crate::mem::frames::{alloc_frame, phys_to_virt};
use mm::paging::{
    PageFlags, entry_frame, entry_present, make_entry, page_table_indices,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    AllocFailed,
    AlreadyMapped,
    NotMapped,
    HugePage,
}

fn table_mut(phys: u64) -> *mut u64 {
    phys_to_virt(phys) as *mut u64
}

/// Returns the physical address of the next-level table for `index`, creating it
/// if absent (when `create`). Fails on a huge-page entry.
unsafe fn next_table(table: *mut u64, index: usize, create: bool) -> Result<Option<u64>, MapError> {
    let slot = unsafe { table.add(index) };
    let entry = unsafe { core::ptr::read_volatile(slot) };
    if entry_present(entry) {
        if PageFlags(entry_flags_bits(entry)).contains(PageFlags::HUGE) {
            return Err(MapError::HugePage);
        }
        return Ok(Some(entry_frame(entry)));
    }
    if !create {
        return Ok(None);
    }
    let frame = alloc_frame().ok_or(MapError::AllocFailed)?;
    // Zero the new table through the HHDM.
    unsafe { core::ptr::write_bytes(table_mut(frame) as *mut u8, 0, 4096) };
    let flags = PageFlags::PRESENT.union(PageFlags::WRITABLE).union(PageFlags::USER);
    unsafe { core::ptr::write_volatile(slot, make_entry(frame, flags)) };
    Ok(Some(frame))
}

fn entry_flags_bits(entry: u64) -> u64 {
    mm::paging::entry_flags(entry).bits()
}

/// # Safety
/// Modifies the active page tables; `virt` must be a page-aligned address not in
/// active use by the kernel, and `phys` a valid frame.
pub unsafe fn map_page(virt: u64, phys: u64, flags: PageFlags) -> Result<(), MapError> {
    let idx = page_table_indices(virt);
    let mut table = table_mut(crate::arch_cr3_base());
    for level in 0..3 {
        let next = unsafe { next_table(table, idx[level], true)?.ok_or(MapError::AllocFailed)? };
        table = table_mut(next);
    }
    let slot = unsafe { table.add(idx[3]) };
    if entry_present(unsafe { core::ptr::read_volatile(slot) }) {
        return Err(MapError::AlreadyMapped);
    }
    unsafe {
        core::ptr::write_volatile(slot, make_entry(phys, flags.union(PageFlags::PRESENT)));
        arch_x86_64::cpu::invlpg(virt);
    }
    Ok(())
}

/// # Safety
/// Modifies the active page tables; `virt` must be a mapped page-aligned address.
pub unsafe fn unmap_page(virt: u64) -> Result<(), MapError> {
    let idx = page_table_indices(virt);
    let mut table = table_mut(crate::arch_cr3_base());
    for level in 0..3 {
        match unsafe { next_table(table, idx[level], false)? } {
            Some(next) => table = table_mut(next),
            None => return Err(MapError::NotMapped),
        }
    }
    let slot = unsafe { table.add(idx[3]) };
    if !entry_present(unsafe { core::ptr::read_volatile(slot) }) {
        return Err(MapError::NotMapped);
    }
    unsafe {
        core::ptr::write_volatile(slot, 0);
        arch_x86_64::cpu::invlpg(virt);
    }
    Ok(())
}

pub fn translate(virt: u64) -> Option<u64> {
    let idx = page_table_indices(virt);
    let mut table = table_mut(crate::arch_cr3_base());
    for level in 0..3 {
        let entry = unsafe { core::ptr::read_volatile(table.add(idx[level])) };
        if !entry_present(entry) {
            return None;
        }
        table = table_mut(entry_frame(entry));
    }
    let entry = unsafe { core::ptr::read_volatile(table.add(idx[3])) };
    if entry_present(entry) {
        Some(entry_frame(entry) + mm::paging::page_offset(virt))
    } else {
        None
    }
}
```

Em `kernel/src/mem/mod.rs`, adicionar `pub mod paging;`.

Em `kernel/src/main.rs`, adicionar um helper que lê a base física do PML4 (CR3 mascarado), já que `map_page`/`translate` o usam:
```rust
/// Physical base of the active PML4 (CR3 with flag bits cleared).
pub fn arch_cr3_base() -> u64 {
    arch_x86_64::cpu::read_cr3() & 0x000F_FFFF_FFFF_F000
}
```

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo xtask ktest`
Expected: `variante ktest ok (9 teste(s))` e as variantes de panic/fault ok.

- [ ] **Step 6: Commit**

```bash
git add crates/arch-x86_64 kernel
git commit -m "feat(kernel): mapeador de paginas de 4 niveis sobre as tabelas ativas

Source: docs Intel SDM Vol. 3A, secao 4.5; CR3/INVLPG
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 5: Heap do kernel — free-list puro e `#[global_allocator]`

**Files:**
- Create: `crates/mm/src/heap.rs`
- Modify: `crates/mm/src/lib.rs` (`pub mod heap;`)
- Create: `kernel/src/mem/heap.rs`
- Modify: `kernel/src/mem/mod.rs` (`pub mod heap;` + init), `kernel/src/main.rs` (chamar init do heap), `kernel/src/ktest.rs` (novos testes)

**Interfaces:**
- Produces (em `crates/mm/src/heap.rs`): `pub struct FreeListHeap` com `pub const fn new() -> Self`; `pub unsafe fn init(&mut self, start: usize, size: usize)`; `pub unsafe fn alloc(&mut self, size: usize, align: usize) -> *mut u8` (nulo ao esgotar); `pub unsafe fn dealloc(&mut self, ptr: *mut u8, size: usize, align: usize)`; `pub fn free_bytes(&self) -> usize`.
- Produces (em `kernel/src/mem/heap.rs`): `pub fn init()` que mapeia a região do heap e inicializa o alocador; `#[global_allocator] static ALLOCATOR: LockedHeap`.

- [ ] **Step 1: Escrever os testes que falham (host, crate `mm`)**

`crates/mm/src/heap.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // A 64 KiB arena on the host heap, 16-byte aligned.
    fn arena() -> (Vec<u64>, usize, usize) {
        let words = 8192; // 64 KiB
        let v = vec![0u64; words];
        let start = v.as_ptr() as usize;
        (v, start, words * 8)
    }

    #[test]
    fn alloc_returns_aligned_nonnull() {
        let (_v, start, size) = arena();
        let mut h = FreeListHeap::new();
        unsafe { h.init(start, size) };
        let p = unsafe { h.alloc(100, 8) };
        assert!(!p.is_null());
        assert_eq!(p as usize % 8, 0);
    }

    #[test]
    fn heap_respects_alignment() {
        let (_v, start, size) = arena();
        let mut h = FreeListHeap::new();
        unsafe { h.init(start, size) };
        let p = unsafe { h.alloc(1, 64) };
        assert!(!p.is_null());
        assert_eq!(p as usize % 64, 0);
    }

    #[test]
    fn heap_returns_null_when_exhausted() {
        let (_v, start, size) = arena();
        let mut h = FreeListHeap::new();
        unsafe { h.init(start, size) };
        let p = unsafe { h.alloc(size * 2, 8) };
        assert!(p.is_null());
    }

    #[test]
    fn heap_reuses_freed_block() {
        let (_v, start, size) = arena();
        let mut h = FreeListHeap::new();
        unsafe { h.init(start, size) };
        let p1 = unsafe { h.alloc(4096, 8) };
        assert!(!p1.is_null());
        unsafe { h.dealloc(p1, 4096, 8) };
        let p2 = unsafe { h.alloc(4096, 8) };
        assert_eq!(p1, p2, "o bloco liberado deveria ser reutilizado");
    }

    #[test]
    fn distinct_allocations_do_not_overlap() {
        let (_v, start, size) = arena();
        let mut h = FreeListHeap::new();
        unsafe { h.init(start, size) };
        let a = unsafe { h.alloc(1000, 8) } as usize;
        let b = unsafe { h.alloc(1000, 8) } as usize;
        assert!(a != 0 && b != 0);
        assert!(a + 1000 <= b || b + 1000 <= a, "alocacoes se sobrepoem");
    }
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test -p mm heap`
Expected: FAIL de compilação — `cannot find type FreeListHeap`.

- [ ] **Step 3: Implementar o free-list (lógica pura)**

Acima dos testes em `crates/mm/src/heap.rs`. É um alocador de lista encadeada de blocos livres, com nós guardados no início de cada bloco livre (first-fit, com split e coalescência por endereço na liberação):
```rust
//! A first-fit free-list heap. Free blocks hold an intrusive `FreeNode` at their
//! start; the list is kept sorted by address so adjacent frees can coalesce.
//! All sizes are rounded up to `align_of::<FreeNode>()` so every block can host a node.

use core::mem::{align_of, size_of};
use core::ptr;

#[repr(C)]
struct FreeNode {
    size: usize,
    next: *mut FreeNode,
}

const NODE_ALIGN: usize = align_of::<FreeNode>();
const NODE_SIZE: usize = size_of::<FreeNode>();

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

pub struct FreeListHeap {
    head: *mut FreeNode,
    free: usize,
}

// The heap is only ever touched behind a lock in the kernel.
unsafe impl Send for FreeListHeap {}

impl FreeListHeap {
    pub const fn new() -> Self {
        Self {
            head: ptr::null_mut(),
            free: 0,
        }
    }

    pub fn free_bytes(&self) -> usize {
        self.free
    }

    /// # Safety
    /// `[start, start+size)` must be a usable, writable, exclusively-owned region.
    pub unsafe fn init(&mut self, start: usize, size: usize) {
        let aligned = align_up(start, NODE_ALIGN);
        let usable = size - (aligned - start);
        if usable < NODE_SIZE {
            return;
        }
        let node = aligned as *mut FreeNode;
        unsafe {
            (*node).size = usable;
            (*node).next = ptr::null_mut();
        }
        self.head = node;
        self.free = usable;
    }

    /// Minimum block size for a request of `size`/`align`: enough to hold the node
    /// and the aligned payload.
    fn block_size(size: usize, align: usize) -> usize {
        let payload = align_up(size, NODE_ALIGN);
        // Worst-case alignment padding is (align - NODE_ALIGN) when align > NODE_ALIGN.
        let pad = align.saturating_sub(NODE_ALIGN);
        align_up(NODE_SIZE + pad + payload, NODE_ALIGN)
    }

    /// # Safety
    /// Standard allocator contract. Returns null when no block fits.
    pub unsafe fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        let need = Self::block_size(size, align.max(NODE_ALIGN));
        let mut prev: *mut *mut FreeNode = &mut self.head;
        unsafe {
            while !(*prev).is_null() {
                let block = *prev;
                let block_addr = block as usize;
                let block_size = (*block).size;
                // Payload must start aligned, after room for a header we can reclaim.
                let payload = align_up(block_addr + NODE_SIZE, align.max(NODE_ALIGN));
                if payload + align_up(size, NODE_ALIGN) <= block_addr + block_size {
                    // Carve the block out. Remainder (if large enough) stays free.
                    let used_end = align_up(payload + align_up(size, NODE_ALIGN), NODE_ALIGN);
                    let remainder = block_addr + block_size - used_end;
                    if remainder >= NODE_SIZE {
                        let rem = used_end as *mut FreeNode;
                        (*rem).size = remainder;
                        (*rem).next = (*block).next;
                        *prev = rem;
                        self.free -= block_size - remainder;
                    } else {
                        *prev = (*block).next;
                        self.free -= block_size;
                    }
                    // Store the real block start/size just below the payload so
                    // dealloc can reconstruct it: write the origin at payload-16.
                    let header = (payload - NODE_SIZE) as *mut FreeNode;
                    (*header).size = used_end - block_addr;
                    (*header).next = block as *mut FreeNode;
                    let _ = need; // need only bounds the search; actual carve is exact
                    return payload as *mut u8;
                }
                prev = &mut (*block).next;
            }
        }
        ptr::null_mut()
    }

    /// # Safety
    /// `ptr` must come from `alloc` with the same `size`/`align`.
    pub unsafe fn dealloc(&mut self, ptr: *mut u8, _size: usize, _align: usize) {
        unsafe {
            let header = (ptr as usize - NODE_SIZE) as *mut FreeNode;
            let origin = (*header).next as usize;
            let total = (*header).size + (ptr as usize - NODE_SIZE - origin + NODE_SIZE);
            // Reconstruct the original block [origin, origin+total) and push it,
            // keeping the list address-sorted with coalescing.
            let block = origin as *mut FreeNode;
            (*block).size = total;
            (*block).next = ptr::null_mut();
            self.insert_sorted(block);
            self.free += total;
        }
    }

    unsafe fn insert_sorted(&mut self, block: *mut FreeNode) {
        unsafe {
            let addr = block as usize;
            let mut prev: *mut *mut FreeNode = &mut self.head;
            while !(*prev).is_null() && (*prev as usize) < addr {
                prev = &mut (**prev).next;
            }
            (*block).next = *prev;
            *prev = block;
            self.coalesce(block);
        }
    }

    unsafe fn coalesce(&mut self, block: *mut FreeNode) {
        unsafe {
            // Merge with next if adjacent.
            let next = (*block).next;
            if !next.is_null() && block as usize + (*block).size == next as usize {
                (*block).size += (*next).size;
                (*block).next = (*next).next;
            }
        }
    }
}
```

Nota de implementação para o implementador: o esquema de cabeçalho acima (guardar origem e tamanho logo abaixo do payload) precisa casar exatamente entre `alloc` e `dealloc`. Se durante a implementação você verificar que o cálculo de `total`/`origin` não reconstrói o bloco original em algum dos testes (em especial `heap_reuses_freed_block` e `distinct_allocations_do_not_overlap`), **ajuste o cabeçalho para guardar diretamente `origin` e `block_size`** em dois campos e reconstrua o bloco a partir deles — o contrato público (`alloc`/`dealloc`/`free_bytes`) não muda, e o objetivo é que os cinco testes passem. Prefira a solução mais simples que passe nos testes; este heap não precisa ser ótimo, só correto.

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test -p mm`
Expected: PASS em `frame`, `paging` e `heap`. Se algum teste de heap falhar, siga a nota acima e simplifique o cabeçalho até os cinco passarem.

- [ ] **Step 5: Integrar no kernel com `#[global_allocator]`**

Em `kernel/src/ktest.rs`, adicionar `("heap_box_and_vec", heap_box_and_vec),` ao fim de `TESTS`, adicionar `extern crate alloc;` no topo do arquivo, e a função:
```rust
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
```

`kernel/src/mem/heap.rs`:
```rust
//! Kernel heap: a fixed virtual window backed by freshly allocated frames, with a
//! `FreeListHeap` behind a spinlock installed as the global allocator.

use core::alloc::{GlobalAlloc, Layout};
use mm::heap::FreeListHeap;
use mm::paging::PageFlags;
use spin::Mutex;

/// Heap virtual window: 2 MiB starting here (above the kernel image, below the
/// HHDM). 512 pages.
const HEAP_START: u64 = 0xffff_ff80_0000_0000;
const HEAP_PAGES: u64 = 512;
const HEAP_SIZE: usize = (HEAP_PAGES * 4096) as usize;

struct LockedHeap(Mutex<FreeListHeap>);

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { self.0.lock().alloc(layout.size(), layout.align()) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.0.lock().dealloc(ptr, layout.size(), layout.align()) }
    }
}

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap(Mutex::new(FreeListHeap::new()));

/// Maps the heap window to fresh frames and initializes the free list.
pub fn init() {
    for page in 0..HEAP_PAGES {
        let virt = HEAP_START + page * 4096;
        let phys = crate::mem::frames::alloc_frame().expect("sem frames para o heap");
        let flags = PageFlags::WRITABLE.union(PageFlags::NO_EXECUTE);
        unsafe {
            crate::mem::paging::map_page(virt, phys, flags).expect("falha ao mapear o heap");
        }
    }
    unsafe { ALLOCATOR.0.lock().init(HEAP_START as usize, HEAP_SIZE) };
}
```

Em `kernel/src/mem/mod.rs`, adicionar `pub mod heap;` e, ao fim de `init()`, **não** chamar o heap (a ordem importa): o heap é inicializado por `kmain` após frames e paging estarem prontos. Para manter uma única entrada, em vez disso faça `mem::init()` chamar, na ordem: `frames::init(...)` (já faz) e então `heap::init()`. Adicione `heap::init();` como última linha de `mem::init()`.

Em `kernel/src/main.rs`, nada muda além do que a Task 2 já adicionou (a chamada `mem::init()` agora também sobe o heap).

- [ ] **Step 6: Rodar e ver passar**

Run: `cargo xtask ktest`
Expected: `variante ktest ok (10 teste(s))` e as variantes de panic/fault ok.

Run: `cargo xtask test`
Expected: PASS (host: `mm`, `arch-x86_64`, `xtask`).

- [ ] **Step 7: Commit**

```bash
git add crates/mm kernel
git commit -m "feat(kernel): heap do kernel com free-list e global_allocator

Source: docs AMD64 APM (paging semantics); Rust GlobalAlloc contract
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

### Task 6: Diagnóstico — panic na tela e backtrace (carryover do Plano 1)

**Files:**
- Modify: `.cargo/config.toml` (frame pointers para o alvo do kernel), `kernel/src/panic.rs`, `kernel/src/main.rs` (variante de teste), `kernel/Cargo.toml` (nova feature de teste), `xtask/src/ktest.rs` (nova variante)

**Interfaces:**
- Consumes: `fbcon::FBCON` (Plano 1), `serial::SERIAL` (Plano 1), `cpu::hlt`.
- Produces: o panic handler também escreve na tela (via `try_lock`) e imprime um backtrace de endereços de retorno; nova feature `ktest-backtrace` e variante de `xtask` que confirma o marcador de backtrace.

- [ ] **Step 1: Habilitar frame pointers para o kernel**

Em `.cargo/config.toml`, no bloco `[target.x86_64-unknown-none]`, acrescentar `"-C", "force-frame-pointers=yes"` à lista `rustflags`:
```toml
[target.x86_64-unknown-none]
rustflags = ["-C", "relocation-model=static", "-C", "code-model=kernel", "-C", "force-frame-pointers=yes"]
```

- [ ] **Step 2: Escrever a variante de teste que falha**

Em `kernel/Cargo.toml`, em `[features]`, adicionar `ktest-backtrace = []`.

Em `kernel/src/main.rs`, após o bloco `ktest-fault`, adicionar:
```rust
    #[cfg(feature = "ktest-backtrace")]
    {
        fn deep(n: u32) {
            if n == 0 {
                panic!("backtrace deliberado");
            }
            deep(n - 1);
            // Prevent tail-call optimization from flattening the frames.
            core::hint::black_box(n);
        }
        deep(3);
    }
```

Em `xtask/src/ktest.rs`, em `run_all`, após a variante `ktest-fault`, acrescentar:
```rust
    run_variant("ktest-backtrace", |o, r| {
        check_expected_panic(o, r, &["backtrace deliberado", "@@TUKOS-BACKTRACE"])
    })?;
```

Nota: `check_expected_panic` verifica a mensagem de panic (`report.panic`) e, aqui, também que a serial contém `@@TUKOS-BACKTRACE`. Como `report.panic` só captura a linha `@@TUKOS-PANIC`, o segundo marcador precisa ser verificado na serial bruta. **Ajuste**: em vez de passar os dois à `check_expected_panic`, verifique o backtrace na serial dentro de `run_variant` ou adicione um parâmetro. Caminho concreto: em `xtask/src/ktest.rs`, substitua a linha acima por um bloco que roda a variante e checa a serial diretamente:
```rust
    {
        let esp = crate::image::build(&["ktest-backtrace"])?;
        let (outcome, serial) = crate::qemu::run_captured(&esp, TIMEOUT)?;
        let report = parse(&serial);
        check_expected_panic(outcome, &report, &["backtrace deliberado"])
            .map_err(|e| format!("variante ktest-backtrace: {e}\n--- serial ---\n{serial}"))?;
        if !serial.contains("@@TUKOS-BACKTRACE") {
            return Err(format!(
                "variante ktest-backtrace: backtrace ausente na serial\n--- serial ---\n{serial}"
            ));
        }
        println!("ktest: variante ktest-backtrace ok");
    }
```

- [ ] **Step 3: Rodar e ver falhar**

Run: `cargo xtask ktest`
Expected: FAIL — a variante `ktest-backtrace` não encontra `@@TUKOS-BACKTRACE` na serial (o panic handler ainda não imprime backtrace).

- [ ] **Step 4: Implementar o panic na tela e o backtrace**

`kernel/src/panic.rs` (substituir o conteúdo):
```rust
use core::fmt::Write;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch_x86_64::cpu::cli();
    // The panic may have interrupted a lock holder; nothing else runs now.
    unsafe { crate::serial::SERIAL.force_unlock() };
    let mut serial = crate::serial::SERIAL.lock();
    let _ = write!(serial, "@@TUKOS-PANIC {}", info.message());
    if let Some(loc) = info.location() {
        let _ = write!(serial, " @ {}:{}", loc.file(), loc.line());
    }
    let _ = writeln!(serial);
    print_backtrace(&mut serial);
    drop(serial);

    // Best-effort copy on screen, so someone watching the QEMU window sees it.
    if let Some(con) = crate::fbcon::FBCON.try_lock().as_mut().and_then(|g| g.as_mut()) {
        let _ = write!(con, "KERNEL PANIC: {}", info.message());
    }

    crate::qemu::exit(crate::qemu::ExitCode::Failure)
}

/// Walks saved RBP frame pointers and prints return addresses. Requires
/// `-C force-frame-pointers=yes`. Bounded to avoid chasing a corrupt chain.
fn print_backtrace(serial: &mut crate::serial::Serial) {
    let mut rbp: u64;
    unsafe { core::arch::asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack)) };
    let _ = writeln!(serial, "@@TUKOS-BACKTRACE");
    for _ in 0..16 {
        if rbp == 0 || rbp & 0x7 != 0 {
            break;
        }
        // frame: [rbp] = saved rbp, [rbp+8] = return address
        let saved_rbp = unsafe { core::ptr::read_volatile(rbp as *const u64) };
        let ret = unsafe { core::ptr::read_volatile((rbp + 8) as *const u64) };
        if ret == 0 {
            break;
        }
        let _ = writeln!(serial, "  ret=0x{ret:x}");
        if saved_rbp <= rbp {
            break; // frames must grow downward in address; stop on anything else
        }
        rbp = saved_rbp;
    }
}
```

Isto exige que `Serial` seja público e nomeável; confirme que `kernel/src/serial.rs` declara `pub struct Serial;` (já declara).

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo xtask ktest`
Expected: todas as variantes ok, incluindo `ktest: variante ktest-backtrace ok`. As variantes `ktest-panic` e `ktest-fault` continuam ok (agora a serial delas também traz `@@TUKOS-BACKTRACE`, o que não quebra os checks existentes).

Run: `cargo xtask run` com `TUKOS_HEADLESS=` (janela visível)
Expected: comportamento normal de boot; o panic só aparece se forçado.

- [ ] **Step 6: Commit**

```bash
git add .cargo/config.toml kernel xtask
git commit -m "feat(kernel): panic na tela e backtrace por frame pointers

Source: docs System V AMD64 ABI (frame pointer chain)
Co-Authored-By: <seu modelo> <noreply@anthropic.com>"
```

---

## Notas de fechamento

- Ao final das 6 tarefas: `cargo fmt --all -- --check`, `cargo clippy -p xtask -p arch-x86_64 -p mm -- -D warnings`, `cargo clippy -p tukos-kernel --target x86_64-unknown-none --features ktest -- -D warnings`, `cargo xtask test` e `cargo xtask ktest` devem passar. Atualize o passo de clippy do host em `.github/workflows/ci.yml` para incluir `-p mm`.
- O ktest `map_translate_unmap` e `map_creates_missing_tables` usam endereços virtuais altos (`0xffff_f000_...`, `0xffff_e800_...`) escolhidos para não colidir com o kernel, o HHDM nem a janela do heap (`0xffff_ff80_...`). Se algum colidir no ambiente real (o HHDM do Limine costuma ficar em `0xffff_8000_...`), escolha outro endereço na metade alta livre e ajuste o teste; registre a mudança.
- Próximo plano (2b), nesta ordem: locks seguros para interrupção (save/restore RFLAGS), `swapgs` + base de GS por CPU + `gdt::set_rsp0`, IST própria para NMI/#MC, teste de double fault por estouro de pilha, timer do LAPIC e habilitar interrupções, scheduler com troca de contexto, modo usuário via SYSCALL/SYSRET, e o alvo `cargo xtask vmware`.
