//! Teclado PS/2 (i8042): poll na batida do timer, decode de scancode set 1 e fila de entrada do console.

use arch_x86_64::interrupts::IrqMutex;
use arch_x86_64::port::inb;
use kbd::{KbdState, RingBuffer};

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const STATUS_OUT_FULL: u8 = 1 << 0;
const STATUS_AUX: u8 = 1 << 5;
/// Maximo de bytes lidos por tick (limita o trabalho no handler).
const MAX_PER_TICK: usize = 8;

static CONSOLE_INPUT: IrqMutex<RingBuffer> = IrqMutex::new(RingBuffer::new());
static STATE: IrqMutex<KbdState> = IrqMutex::new(KbdState { shift: false });

/// Decodifica um byte de scancode e, se gerar um caractere, enfileira-o.
pub fn push_scancode_byte(sc: u8) {
    let ch = kbd::decode(sc, &mut STATE.lock());
    if let Some(c) = ch {
        CONSOLE_INPUT.lock().push(c);
    }
}

/// Retira um byte da fila de entrada do console, sem bloquear.
pub fn try_pop_input() -> Option<u8> {
    CONSOLE_INPUT.lock().pop()
}

/// Drena o i8042 (chamado a cada tick do timer).
pub fn poll() {
    for _ in 0..MAX_PER_TICK {
        let status = unsafe { inb(STATUS_PORT) };
        if status & STATUS_OUT_FULL == 0 {
            break;
        }
        let byte = unsafe { inb(DATA_PORT) };
        if status & STATUS_AUX != 0 {
            continue; // byte de mouse: descarta
        }
        push_scancode_byte(byte);
    }
}

/// Pre-enche a fila de entrada com bytes sinteticos (testes).
#[cfg(feature = "ktest")]
pub fn test_push_input(bytes: &[u8]) {
    let mut q = CONSOLE_INPUT.lock();
    for &b in bytes {
        q.push(b);
    }
}
