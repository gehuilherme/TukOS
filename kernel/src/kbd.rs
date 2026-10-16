//! Teclado PS/2 (i8042): poll na batida do timer, decode de scancode set 1 e fila de entrada do console.

use arch_x86_64::interrupts::IrqMutex;
use arch_x86_64::port::{inb, outb};
use kbd::{InputEvent, InputRing, KbdState, KeyEvent, MouseDecoder, MouseState, RingBuffer};

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const STATUS_OUT_FULL: u8 = 1 << 0;
const STATUS_AUX: u8 = 1 << 5;
/// Maximo de bytes lidos por tick (limita o trabalho no handler).
const MAX_PER_TICK: usize = 8;

static CONSOLE_INPUT: IrqMutex<RingBuffer> = IrqMutex::new(RingBuffer::new());
/// Fila UNIFICADA de eventos de entrada crus (teclado + mouse, GUI); limitada,
/// descarta o mais antigo quando cheia.
static INPUT_EVENTS: IrqMutex<InputRing> = IrqMutex::new(InputRing::new());
static MOUSE_DEC: IrqMutex<MouseDecoder> = IrqMutex::new(MouseDecoder::new());
static MOUSE: IrqMutex<MouseState> = IrqMutex::new(MouseState::new());
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
        INPUT_EVENTS.lock().push(e.into());
    }
}

/// Retira o proximo evento de TECLA da fila unificada, sem bloquear. Eventos de
/// mouse que o precedem sao descartados (consumidores do nr 17 so querem teclas).
pub fn try_pop_key_event() -> Option<KeyEvent> {
    INPUT_EVENTS.lock().pop_key()
}

/// Retira o proximo evento de entrada (tecla OU mouse), sem bloquear.
pub fn try_pop_input_event() -> Option<InputEvent> {
    INPUT_EVENTS.lock().pop()
}

/// Um byte de mouse: monta o pacote, atualiza `MOUSE` e empurra os eventos.
pub fn push_mouse_byte(b: u8) {
    let Some(pkt) = MOUSE_DEC.lock().feed(b) else {
        return;
    };
    let (w, h) = crate::fbcon::fb_info()
        .map(|(w, h, _)| (w as i32, h as i32))
        .unwrap_or((i32::MAX, i32::MAX));
    let evs = MOUSE.lock().apply(pkt, w, h);
    let mut q = INPUT_EVENTS.lock();
    for e in evs.into_iter().flatten() {
        q.push(e);
    }
}

/// Roteia um byte do i8042 pelo bit AUXDATA.
pub fn route_byte(byte: u8, aux: bool) {
    if aux {
        push_mouse_byte(byte);
    } else {
        push_scancode_byte(byte);
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
        route_byte(byte, status & STATUS_AUX != 0);
    }
}

const STATUS_IN_FULL: u8 = 1 << 1;
const CMD_ENABLE_AUX: u8 = 0xA8;
const CMD_WRITE_AUX: u8 = 0xD4;
const MOUSE_ENABLE_REPORTING: u8 = 0xF4;
const MOUSE_ACK: u8 = 0xFA;
/// Limite de iteracoes de espera (mouse ausente nao pode travar o boot).
const WAIT_SPINS: u32 = 100_000;

fn wait_input_empty() -> bool {
    for _ in 0..WAIT_SPINS {
        if unsafe { inb(STATUS_PORT) } & STATUS_IN_FULL == 0 {
            return true;
        }
    }
    false
}

/// Espera um byte no buffer de saida e o roteia ate achar um do mouse (AUX);
/// bytes de teclado nesse meio tempo vao ao teclado. Devolve o byte do mouse.
fn read_aux_byte() -> Option<u8> {
    for _ in 0..WAIT_SPINS {
        let status = unsafe { inb(STATUS_PORT) };
        if status & STATUS_OUT_FULL == 0 {
            continue;
        }
        let b = unsafe { inb(DATA_PORT) };
        if status & STATUS_AUX != 0 {
            return Some(b);
        }
        push_scancode_byte(b);
    }
    None
}

/// Habilita o dispositivo auxiliar (mouse) do i8042 e o reporte de dados.
/// Usamos polling (sem IRQ12), entao os bits de interrupcao do controlador
/// ficam como estao. Esperas limitadas: sem mouse, retorna false sem travar.
pub fn init_mouse() -> bool {
    if !wait_input_empty() {
        return false;
    }
    unsafe { outb(STATUS_PORT, CMD_ENABLE_AUX) };
    if !wait_input_empty() {
        return false;
    }
    unsafe { outb(STATUS_PORT, CMD_WRITE_AUX) };
    if !wait_input_empty() {
        return false;
    }
    unsafe { outb(DATA_PORT, MOUSE_ENABLE_REPORTING) };
    // ACK 0xFA do mouse; qualquer outro byte de mouse e ignorado.
    for _ in 0..4 {
        match read_aux_byte() {
            Some(MOUSE_ACK) => return true,
            Some(_) => {}
            None => return false,
        }
    }
    false
}

/// Pre-enche a fila de entrada com bytes sinteticos (testes).
#[cfg(feature = "ktest")]
pub fn test_push_input(bytes: &[u8]) {
    let mut q = CONSOLE_INPUT.lock();
    for &b in bytes {
        q.push(b);
    }
}
