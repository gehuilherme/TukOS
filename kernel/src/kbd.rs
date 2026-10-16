//! Teclado PS/2 (i8042): poll na batida do timer, decode de scancode set 1 e fila de entrada do console.

use arch_x86_64::interrupts::IrqMutex;
use arch_x86_64::port::inb;
use kbd::{EventRing, KbdState, KeyEvent, RingBuffer};

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const STATUS_OUT_FULL: u8 = 1 << 0;
const STATUS_AUX: u8 = 1 << 5;
/// Maximo de bytes lidos por tick (limita o trabalho no handler).
const MAX_PER_TICK: usize = 8;

static CONSOLE_INPUT: IrqMutex<RingBuffer> = IrqMutex::new(RingBuffer::new());
/// Fila de eventos crus de tecla (GUI); limitada, descarta o mais antigo quando cheia.
static KEY_EVENTS: IrqMutex<EventRing> = IrqMutex::new(EventRing::new());
static STATE: IrqMutex<KbdState> = IrqMutex::new(KbdState { shift: false });

/// Decodifica um byte de scancode: alimenta o anel de caracteres do console E a
/// fila de eventos crus de tecla (o mesmo estado de shift serve aos dois).
pub fn push_scancode_byte(sc: u8) {
    let (ch, ev) = {
        let mut st = STATE.lock();
        (kbd::decode(sc, &mut st), kbd::decode_event(sc, &mut st))
    };
    if let Some(c) = ch {
        CONSOLE_INPUT.lock().push(c);
    }
    if let Some(e) = ev {
        KEY_EVENTS.lock().push(e);
    }
}

/// Retira um evento de tecla da fila, sem bloquear.
pub fn try_pop_key_event() -> Option<KeyEvent> {
    KEY_EVENTS.lock().pop()
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
