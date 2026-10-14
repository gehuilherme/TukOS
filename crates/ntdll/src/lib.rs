//! TukOS ntdll.dll: thin syscall stubs with the Windows x64 convention, plus
//! `LdrInitializeThunk`, the user-mode import loader.
//! Each `Nt*` export does `mov r10, rcx; mov eax, <tukos ssn>; syscall; ret`.
//! The syscall numbers MUST match the TukOS kernel dispatch.
//!
//! The DLL has no imports, no heap and no CRT: everything it needs is in `core`,
//! the alloc-free accessors of the `pe` crate, and the `mem*` primitives below.
#![no_std]
#![allow(non_snake_case)]

use core::panic::PanicInfo;
use pe::{PeFile, SCN_MEM_EXECUTE, SCN_MEM_WRITE};

/// A panic can only come from a latent bug; never unwind, just end the process.
#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    die(ST_PANIC)
}

// ---------------------------------------------------------------------------
// No-CRT memory primitives. The compiler may emit calls to these even in
// freestanding code; they use `rep` string ops / volatile accesses so the
// optimizer cannot turn them back into calls to themselves.
// ---------------------------------------------------------------------------

/// # Safety
/// Standard `memcpy` contract: `dst`/`src` valid for `n` bytes, non-overlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        core::arch::asm!(
            "rep movsb",
            inout("rcx") n => _,
            inout("rdi") dst => _,
            inout("rsi") src => _,
            options(nostack, preserves_flags)
        );
    }
    dst
}

/// # Safety
/// Standard `memset` contract: `dst` valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dst: *mut u8, c: i32, n: usize) -> *mut u8 {
    unsafe {
        core::arch::asm!(
            "rep stosb",
            inout("rcx") n => _,
            inout("rdi") dst => _,
            in("al") c as u8,
            options(nostack, preserves_flags)
        );
    }
    dst
}

/// # Safety
/// Standard `memmove` contract: both pointers valid for `n` bytes (may overlap).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        if (dst as usize) <= (src as usize) || (dst as usize) >= (src as usize).wrapping_add(n) {
            let mut i = 0;
            while i < n {
                core::ptr::write_volatile(dst.add(i), core::ptr::read_volatile(src.add(i)));
                i += 1;
            }
        } else {
            let mut i = n;
            while i > 0 {
                i -= 1;
                core::ptr::write_volatile(dst.add(i), core::ptr::read_volatile(src.add(i)));
            }
        }
    }
    dst
}

/// # Safety
/// Standard `memcmp` contract: both pointers valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let (x, y) = unsafe {
            (
                core::ptr::read_volatile(a.add(i)),
                core::ptr::read_volatile(b.add(i)),
            )
        };
        if x != y {
            return i32::from(x) - i32::from(y);
        }
        i += 1;
    }
    0
}

/// # Safety
/// Same contract as `memcmp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    unsafe { memcmp(a, b, n) }
}

// Linker symbols the MSVC toolchain expects from `core`/`compiler_builtins` even
// though this DLL never uses floats or C++ exceptions (/NODEFAULTLIB: no CRT).
#[unsafe(no_mangle)]
static _fltused: i32 = 0;

#[unsafe(no_mangle)]
extern "C" fn __CxxFrameHandler3() -> i32 {
    0
}

// TukOS syscall numbers (must match kernel/src/syscall.rs).
// NT_WRITE_FILE = 2, NT_TERMINATE_PROCESS = 3, NT_CLOSE = 4.

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn NtWriteFile() {
    core::arch::naked_asm!("mov r10, rcx", "mov eax, 2", "syscall", "ret");
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn NtTerminateProcess() {
    core::arch::naked_asm!("mov r10, rcx", "mov eax, 3", "syscall", "ret");
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn NtClose() {
    core::arch::naked_asm!("mov r10, rcx", "mov eax, 4", "syscall", "ret");
}

// ---------------------------------------------------------------------------
// LdrInitializeThunk: the user-mode import loader.
// ---------------------------------------------------------------------------

// Distinct NtTerminateProcess statuses for every way the loader can fail.
const ST_NO_PEB: u32 = 0xC000_7001;
const ST_NO_EXE: u32 = 0xC000_7002;
const ST_NO_NTDLL: u32 = 0xC000_7003;
const ST_BAD_PE: u32 = 0xC000_7004;
const ST_UNRESOLVED: u32 = 0xC000_7005;
/// Must match `LDR_ST_BAD_IAT` in kernel/src/ktest.rs.
const ST_BAD_IAT: u32 = 0xC000_7006;
const ST_BAD_ENTRY: u32 = 0xC000_7007;
const ST_PANIC: u32 = 0xC000_7008;
/// An imported DLL (or forwarder target DLL) is not in `PEB->Ldr`.
const ST_NO_DEP: u32 = 0xC000_7009;
/// A forwarder chain exceeded `MAX_FORWARD_HOPS` (cycle or absurd depth).
const ST_FWD_LIMIT: u32 = 0xC000_700A;

/// NtTerminateProcess(status) via syscall; never returns.
fn die(status: u32) -> ! {
    loop {
        unsafe {
            core::arch::asm!(
                "syscall",
                in("eax") 3u32,
                in("r10") u64::from(status),
                out("rcx") _,
                out("r11") _,
                options(nostack)
            );
        }
    }
}

// TEB / PEB / PEB_LDR_DATA / LDR_DATA_TABLE_ENTRY offsets (public winternl.h layout).
const TEB_SELF: usize = 0x30;
const TEB_PEB: usize = 0x60;
const PEB_LDR: usize = 0x18;
const LDR_IN_LOAD_ORDER: usize = 0x10;
const ENT_DLL_BASE: usize = 0x30;
const ENT_SIZE_OF_IMAGE: usize = 0x40;
const ENT_BASE_NAME_LEN: usize = 0x58;
const ENT_BASE_NAME_BUF: usize = 0x60;
const MAX_MODULES: usize = 16;

/// Max forwarder hops followed per import; a longer chain (or a cycle) dies with
/// `ST_FWD_LIMIT` instead of looping.
const MAX_FORWARD_HOPS: usize = 8;
const NAME_CAP: usize = 24;

/// One module of `PEB->Ldr`, as the loader sees it. `name` is the lowercase ASCII
/// BaseDllName (`nlen` bytes, `.dll` suffix stripped; `nlen == 0` if it is not usable,
/// which then never matches an import).
#[derive(Clone, Copy)]
struct Module {
    base: usize,
    size: usize,
    name: [u8; NAME_CAP],
    nlen: usize,
}

const NO_MODULE: Module = Module {
    base: 0,
    size: 0,
    name: [0; NAME_CAP],
    nlen: 0,
};

impl Module {
    /// Parses the module as a LOADED image (sections at their RVAs).
    ///
    /// # Safety
    /// `base..base+size` must be mapped and readable (guaranteed by the kernel's
    /// `PEB->Ldr` entry).
    unsafe fn pe(&self) -> Option<PeFile<'static>> {
        if self.base == 0 || self.size == 0 {
            return None;
        }
        let img = unsafe { core::slice::from_raw_parts(self.base as *const u8, self.size) };
        PeFile::parse_loaded(img).ok()
    }
}

/// `name` without a trailing `.dll` (ASCII case-insensitive).
fn strip_dll(name: &[u8]) -> &[u8] {
    if name.len() >= 4 && name[name.len() - 4..].eq_ignore_ascii_case(b".dll") {
        &name[..name.len() - 4]
    } else {
        name
    }
}

/// Index of the module whose base name equals `dll` ("kernel32.dll", "KERNEL32" and
/// "Kernel32.DLL" all match): ASCII case-insensitive, `.dll` suffix ignored on both sides.
fn find_module(mods: &[Module], dll: &str) -> Option<usize> {
    let want = strip_dll(dll.as_bytes());
    if want.is_empty() {
        return None;
    }
    mods.iter().position(|m| {
        m.nlen != 0 && m.nlen == want.len() && m.name[..m.nlen].eq_ignore_ascii_case(want)
    })
}

/// Reads the UTF-16 `BaseDllName` of an LDR entry into a lowercase, `.dll`-stripped
/// ASCII buffer. Returns `(buf, len)`; len 0 for names that are empty, non-ASCII or
/// too long.
///
/// # Safety
/// `entry` must point to a readable LDR_DATA_TABLE_ENTRY.
unsafe fn read_base_name(entry: *const u8) -> ([u8; NAME_CAP], usize) {
    unsafe {
        let mut out = [0u8; NAME_CAP];
        let bytes = core::ptr::read(entry.add(ENT_BASE_NAME_LEN) as *const u16) as usize;
        let buf = core::ptr::read(entry.add(ENT_BASE_NAME_BUF) as *const *const u16);
        let chars = bytes / 2;
        if buf.is_null() || chars == 0 || chars > NAME_CAP {
            return (out, 0);
        }
        let mut i = 0;
        while i < chars {
            let c = core::ptr::read(buf.add(i));
            if c == 0 || c > 0x7F {
                return ([0; NAME_CAP], 0);
            }
            out[i] = (c as u8).to_ascii_lowercase();
            i += 1;
        }
        let len = strip_dll(&out[..chars]).len();
        (out, len)
    }
}

/// The 8-byte IAT slot at `rva` must lie entirely inside one section of the IMPORTING
/// module (`importer`, whose SizeOfImage is `size_of_image`) that is non-executable and
/// writable, or lies in the importer's own declared IAT directory (MSVC's read-only
/// `.rdata` IAT; the kernel maps those pages writable). This is the per-module
/// confinement invariant: whichever module's imports are being bound, the loader can
/// only write inside THAT module's image, never into code, never into another module.
/// The `!exec` gate holds even if a hostile data directory #12 covers a code section.
fn iat_slot_ok(importer: &PeFile, size_of_image: u64, rva: u32) -> bool {
    let start = u64::from(rva);
    let Some(end) = start.checked_add(8) else {
        return false;
    };
    if end > size_of_image {
        return false;
    }
    let (iat_rva, iat_size) = importer.iat_directory();
    let iat_start = u64::from(iat_rva);
    let iat_end = iat_start + u64::from(iat_size);
    let in_iat_dir = iat_size != 0 && start >= iat_start && end <= iat_end;
    importer.sections().any(|s| {
        let s_start = u64::from(s.rva);
        let s_end = s_start + u64::from(s.virtual_size);
        let inside = start >= s_start && end <= s_end;
        let exec = s.characteristics & SCN_MEM_EXECUTE != 0;
        inside && !exec && (s.characteristics & SCN_MEM_WRITE != 0 || in_iat_dir)
    })
}

/// Resolves `entry` against module `exporter` to an absolute address, chasing
/// forwarder exports ("DLL.Func" / "DLL.#N") through the module list for at most
/// `MAX_FORWARD_HOPS` forwards. Err carries the sentinel to die with.
///
/// # Safety
/// Every module in `mods` must describe a mapped, readable image.
unsafe fn resolve_import(
    mods: &[Module],
    exporter: usize,
    entry: &pe::ImportEntry,
) -> Result<usize, u32> {
    let mut idx = exporter;
    let mut target: Option<pe::ForwardTarget> = None; // None on the first hop: use `entry`
    let mut hops = 0;
    loop {
        let m = mods.get(idx).ok_or(ST_UNRESOLVED)?;
        let exp = unsafe { m.pe() }.ok_or(ST_BAD_PE)?;
        let r = match target {
            None => exp.resolve_entry_ex(entry),
            Some(pe::ForwardTarget::Name(n)) => exp.resolve_export_ex(n),
            Some(pe::ForwardTarget::Ordinal(o)) => exp.resolve_export_ordinal_ex(o),
        };
        match r.ok_or(ST_UNRESOLVED)? {
            pe::Resolved::Rva(rva) => {
                if rva as usize >= m.size {
                    return Err(ST_UNRESOLVED);
                }
                return m.base.checked_add(rva as usize).ok_or(ST_UNRESOLVED);
            }
            pe::Resolved::Forwarder(s) => {
                hops += 1;
                if hops > MAX_FORWARD_HOPS {
                    return Err(ST_FWD_LIMIT);
                }
                let (dll, t) = pe::split_forwarder(s).ok_or(ST_UNRESOLVED)?;
                idx = find_module(mods, dll).ok_or(ST_NO_DEP)?;
                target = Some(t);
            }
        }
    }
}

/// Rust body of the thunk (the naked entry below aligns the stack first).
extern "C" fn ldr_main() -> ! {
    unsafe {
        // TEB via GS (NtTib.Self), then PEB, then PEB->Ldr.
        let teb: usize;
        core::arch::asm!("mov {}, gs:[{off}]", out(reg) teb, off = const TEB_SELF,
            options(nostack, readonly, preserves_flags));
        if teb == 0 {
            die(ST_NO_PEB);
        }
        let peb = core::ptr::read((teb + TEB_PEB) as *const usize);
        if peb == 0 {
            die(ST_NO_PEB);
        }
        let ldr = core::ptr::read((peb + PEB_LDR) as *const usize);
        if ldr == 0 {
            die(ST_NO_PEB);
        }

        // Walk InLoadOrderModuleList into a small fixed array (first entry = the exe).
        let head = (ldr + LDR_IN_LOAD_ORDER) as *const usize;
        let mut cur = core::ptr::read(head) as *const u8;
        if core::ptr::eq(cur, head as *const u8) || cur.is_null() {
            die(ST_NO_EXE);
        }
        let mut mods = [NO_MODULE; MAX_MODULES];
        let mut n = 0;
        while !core::ptr::eq(cur, head as *const u8) && !cur.is_null() && n < MAX_MODULES {
            let (name, nlen) = read_base_name(cur);
            mods[n] = Module {
                base: core::ptr::read(cur.add(ENT_DLL_BASE) as *const usize),
                size: core::ptr::read(cur.add(ENT_SIZE_OF_IMAGE) as *const u32) as usize,
                name,
                nlen,
            };
            n += 1;
            cur = core::ptr::read(cur as *const usize) as *const u8;
        }
        let mods = &mods[..n];
        if find_module(mods, "ntdll.dll").is_none() {
            die(ST_NO_NTDLL);
        }
        let exe = mods[0];
        if exe.base == 0 || exe.size == 0 {
            die(ST_BAD_PE);
        }

        // Bind the imports of EVERY module, dependencies first: reverse load order
        // (ntdll, kernel32, exe). The address written into an IAT is the exporter's
        // final export address, so it does not depend on the exporter's own imports
        // being bound yet; the order only makes failures surface dependency-first.
        let mut i = n;
        while i > 0 {
            i -= 1;
            let imp = mods[i];
            let Some(importer) = imp.pe() else {
                die(ST_BAD_PE)
            };
            let Ok(imports) = importer.imports() else {
                die(ST_BAD_PE)
            };
            for dll in imports {
                let Some(ex) = find_module(mods, dll.name) else {
                    die(ST_NO_DEP)
                };
                for entry in dll.thunks {
                    let addr = match resolve_import(mods, ex, &entry) {
                        Ok(a) => a,
                        Err(st) => die(st),
                    };
                    // Confine the write to THIS importer's own image.
                    if !iat_slot_ok(&importer, imp.size as u64, entry.iat_rva) {
                        die(ST_BAD_IAT);
                    }
                    let slot = (imp.base + entry.iat_rva as usize) as *mut u64;
                    core::ptr::write_volatile(slot, addr as u64);
                }
            }
        }

        let Some(exe_pe) = exe.pe() else {
            die(ST_BAD_PE)
        };
        let ep = exe_pe.entry_point_rva() as usize;
        if ep == 0 || ep >= exe.size {
            die(ST_BAD_ENTRY);
        }
        let entry: extern "system" fn() -> ! = core::mem::transmute(exe.base + ep);
        entry()
    }
}

/// Initial user-mode entry of every process: aligns the stack to the x64 ABI
/// (16 bytes at the call + 32 bytes of shadow space) and runs the loader.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "system" fn LdrInitializeThunk() -> ! {
    core::arch::naked_asm!(
        "and rsp, -16",
        "sub rsp, 32",
        "call {main}",
        "ud2",
        main = sym ldr_main,
    );
}
