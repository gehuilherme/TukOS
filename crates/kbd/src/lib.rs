//! Decodificacao de scancodes PS/2 (set 1, layout US) e buffer circular de entrada do console.
//! Logica pura, testavel no host.
#![no_std]

/// Estado do decodificador (teclas modificadoras).
#[derive(Default, Debug, Clone, Copy)]
pub struct KbdState {
    pub shift: bool,
}

/// Tabelas indexadas por scancode de make (0x00..=0x39); 0 = sem caractere.
const fn build(rows: &[(usize, &[u8])]) -> [u8; 0x3A] {
    let mut t = [0u8; 0x3A];
    let mut r = 0;
    while r < rows.len() {
        let (start, s) = rows[r];
        let mut i = 0;
        while i < s.len() {
            t[start + i] = s[i];
            i += 1;
        }
        r += 1;
    }
    t
}

static NORMAL: [u8; 0x3A] = build(&[
    (0x02, b"1234567890-="),
    (0x0E, b"\x08\t"),
    (0x10, b"qwertyuiop[]\n"),
    (0x1E, b"asdfghjkl;'`"),
    (0x2B, b"\\zxcvbnm,./"),
    (0x37, b"*"),
    (0x39, b" "),
]);

static SHIFTED: [u8; 0x3A] = build(&[
    (0x02, b"!@#$%^&*()_+"),
    (0x0E, b"\x08\t"),
    (0x10, b"QWERTYUIOP{}\n"),
    (0x1E, b"ASDFGHJKL:\"~"),
    (0x2B, b"|ZXCVBNM<>?"),
    (0x37, b"*"),
    (0x39, b" "),
]);

/// Decodifica um byte de scancode (set 1). Retorna o byte ASCII produzido, se houver.
pub fn decode(scancode: u8, st: &mut KbdState) -> Option<u8> {
    match scancode {
        0x2A | 0x36 => {
            st.shift = true;
            return None;
        }
        0xAA | 0xB6 => {
            st.shift = false;
            return None;
        }
        _ => {}
    }
    if scancode & 0x80 != 0 {
        return None;
    }
    let table = if st.shift { &SHIFTED } else { &NORMAL };
    match table.get(scancode as usize).copied() {
        Some(0) | None => None,
        Some(c) => Some(c),
    }
}

/// Evento de tecla cru (make/break) com codigo de tecla virtual publico (winuser.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyEvent {
    /// VK_* (0 = sem mapeamento).
    pub vk: u16,
    /// Caractere gerado (so em `down`; 0 se nenhum). Enter gera 0x0D, como WM_CHAR.
    pub ch: u32,
    pub down: bool,
}

pub const VK_BACK: u16 = 0x08;
pub const VK_TAB: u16 = 0x09;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_SHIFT: u16 = 0x10;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SPACE: u16 = 0x20;

/// Codigo virtual de um scancode de make (set 1), derivado do caractere sem shift.
fn vk_for(make: u8) -> u16 {
    match make {
        0x01 => return VK_ESCAPE,
        0x0E => return VK_BACK,
        0x0F => return VK_TAB,
        0x1C => return VK_RETURN,
        0x39 => return VK_SPACE,
        0x37 => return 0x6A, // VK_MULTIPLY
        _ => {}
    }
    match NORMAL.get(make as usize).copied() {
        Some(c @ b'a'..=b'z') => (c - 32) as u16,
        Some(c @ b'0'..=b'9') => c as u16,
        Some(b';') => 0xBA,
        Some(b'=') => 0xBB,
        Some(b',') => 0xBC,
        Some(b'-') => 0xBD,
        Some(b'.') => 0xBE,
        Some(b'/') => 0xBF,
        Some(b'`') => 0xC0,
        Some(b'[') => 0xDB,
        Some(b'\\') => 0xDC,
        Some(b']') => 0xDD,
        Some(b'\'') => 0xDE,
        _ => 0,
    }
}

/// Decodifica um byte de scancode (set 1) em evento de tecla. Atualiza o estado de shift.
/// Bytes sem mapeamento (prefixo 0xE0, teclas desconhecidas) retornam `None`.
pub fn decode_event(byte: u8, st: &mut KbdState) -> Option<KeyEvent> {
    let down = byte & 0x80 == 0;
    let make = byte & 0x7F;
    if make == 0x2A || make == 0x36 {
        st.shift = down;
        return Some(KeyEvent {
            vk: VK_SHIFT,
            ch: 0,
            down,
        });
    }
    let vk = vk_for(make);
    if vk == 0 {
        return None;
    }
    let ch = if down {
        let table = if st.shift { &SHIFTED } else { &NORMAL };
        match table.get(make as usize).copied() {
            Some(b'\n') => 0x0D,
            Some(c) => c as u32,
            None => 0,
        }
    } else {
        0
    };
    Some(KeyEvent { vk, ch, down })
}

/// FIFO limitada de eventos. Cheia: descarta o evento mais ANTIGO.
pub struct EventRing {
    buf: [KeyEvent; Self::CAP],
    head: usize,
    len: usize,
}

impl EventRing {
    pub const CAP: usize = 64;

    pub const fn new() -> Self {
        Self {
            buf: [KeyEvent {
                vk: 0,
                ch: 0,
                down: false,
            }; Self::CAP],
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, e: KeyEvent) {
        if self.len == Self::CAP {
            self.head = (self.head + 1) % Self::CAP;
            self.len -= 1;
        }
        self.buf[(self.head + self.len) % Self::CAP] = e;
        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<KeyEvent> {
        if self.len == 0 {
            return None;
        }
        let e = self.buf[self.head];
        self.head = (self.head + 1) % Self::CAP;
        self.len -= 1;
        Some(e)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for EventRing {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Mouse PS/2 (pacote padrao de 3 bytes) e eventos de entrada unificados.
// ---------------------------------------------------------------------------

/// Pacote de mouse decodificado. `dy` e positivo para CIMA (convencao PS/2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MousePacket {
    pub dx: i16,
    pub dy: i16,
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

/// Acumulador de bytes -> pacote. Ressincroniza pelo bit 3 do byte 0 (sempre 1).
#[derive(Debug, Clone, Copy, Default)]
pub struct MouseDecoder {
    buf: [u8; 3],
    n: usize,
}

impl MouseDecoder {
    pub const fn new() -> Self {
        Self { buf: [0; 3], n: 0 }
    }

    /// Alimenta um byte; devolve um pacote quando completa 3 bytes validos.
    /// Overflow (bits 6/7 do byte 0) satura o delta em +-255.
    pub fn feed(&mut self, byte: u8) -> Option<MousePacket> {
        if self.n == 0 && byte & 0x08 == 0 {
            return None; // byte 0 invalido: descarta e ressincroniza
        }
        self.buf[self.n] = byte;
        self.n += 1;
        if self.n < 3 {
            return None;
        }
        self.n = 0;
        let b0 = self.buf[0];
        let axis = |raw: u8, neg: bool, ovf: bool| -> i16 {
            if ovf {
                if neg { -255 } else { 255 }
            } else if neg {
                raw as i16 - 256
            } else {
                raw as i16
            }
        };
        Some(MousePacket {
            dx: axis(self.buf[1], b0 & 0x10 != 0, b0 & 0x40 != 0),
            dy: axis(self.buf[2], b0 & 0x20 != 0, b0 & 0x80 != 0),
            left: b0 & 1 != 0,
            right: b0 & 2 != 0,
            middle: b0 & 4 != 0,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Move,
    Down(MouseButton),
    Up(MouseButton),
}

/// Evento de entrada unificado (teclado + mouse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Key {
        vk: u16,
        ch: u32,
        down: bool,
    },
    Mouse {
        x: i32,
        y: i32,
        buttons: u8,
        kind: MouseKind,
    },
}

impl From<KeyEvent> for InputEvent {
    fn from(k: KeyEvent) -> Self {
        InputEvent::Key {
            vk: k.vk,
            ch: k.ch,
            down: k.down,
        }
    }
}

/// Posicao + botoes (bit0 esq, bit1 dir, bit2 meio) do cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub buttons: u8,
}

impl MouseState {
    pub const fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            buttons: 0,
        }
    }

    /// Aplica um pacote: x += dx, y -= dy (tela cresce para baixo), clamp a
    /// `[0,w) x [0,h)`. Devolve ate 4 eventos: Move (se houve delta) e depois
    /// Down/Up por botao alterado (esq, dir, meio).
    pub fn apply(&mut self, p: MousePacket, w: i32, h: i32) -> [Option<InputEvent>; 4] {
        let mut out = [None; 4];
        let mut n = 0;
        self.x = self.x.saturating_add(p.dx as i32).clamp(0, (w - 1).max(0));
        self.y = self.y.saturating_sub(p.dy as i32).clamp(0, (h - 1).max(0));
        let new = p.left as u8 | (p.right as u8) << 1 | (p.middle as u8) << 2;
        let old = self.buttons;
        self.buttons = new;
        let mk = |s: &Self, kind| InputEvent::Mouse {
            x: s.x,
            y: s.y,
            buttons: s.buttons,
            kind,
        };
        if p.dx != 0 || p.dy != 0 {
            out[n] = Some(mk(self, MouseKind::Move));
            n += 1;
        }
        for (bit, b) in [
            (1u8, MouseButton::Left),
            (2, MouseButton::Right),
            (4, MouseButton::Middle),
        ] {
            if (old ^ new) & bit != 0 {
                let kind = if new & bit != 0 {
                    MouseKind::Down(b)
                } else {
                    MouseKind::Up(b)
                };
                out[n] = Some(mk(self, kind));
                n += 1;
            }
        }
        out
    }
}

/// FIFO limitada de `InputEvent`. Cheia: descarta o evento mais ANTIGO.
pub struct InputRing {
    buf: [Option<InputEvent>; Self::CAP],
    head: usize,
    len: usize,
}

impl InputRing {
    pub const CAP: usize = 64;

    pub const fn new() -> Self {
        Self {
            buf: [None; Self::CAP],
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, e: InputEvent) {
        if self.len == Self::CAP {
            self.head = (self.head + 1) % Self::CAP;
            self.len -= 1;
        }
        self.buf[(self.head + self.len) % Self::CAP] = Some(e);
        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<InputEvent> {
        if self.len == 0 {
            return None;
        }
        let e = self.buf[self.head].take();
        self.head = (self.head + 1) % Self::CAP;
        self.len -= 1;
        e
    }

    /// Retira o proximo evento de TECLA, descartando eventos de mouse que o precedem.
    pub fn pop_key(&mut self) -> Option<KeyEvent> {
        while let Some(e) = self.pop() {
            if let InputEvent::Key { vk, ch, down } = e {
                return Some(KeyEvent { vk, ch, down });
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for InputRing {
    fn default() -> Self {
        Self::new()
    }
}

/// FIFO de bytes de tamanho fixo. Cheio: descarta o byte mais novo.
pub struct RingBuffer {
    buf: [u8; Self::CAP],
    head: usize,
    len: usize,
}

impl RingBuffer {
    pub const CAP: usize = 256;

    pub const fn new() -> Self {
        Self {
            buf: [0; Self::CAP],
            head: 0,
            len: 0,
        }
    }

    /// Retorna false se o byte foi descartado (buffer cheio).
    pub fn push(&mut self, b: u8) -> bool {
        if self.len == Self::CAP {
            return false;
        }
        self.buf[(self.head + self.len) % Self::CAP] = b;
        self.len += 1;
        true
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        let b = self.buf[self.head];
        self.head = (self.head + 1) % Self::CAP;
        self.len -= 1;
        Some(b)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for RingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_shift() {
        let mut s = KbdState::default();
        assert_eq!(decode(0x1E, &mut s), Some(b'a'));
        assert_eq!(decode(0x2A, &mut s), None);
        assert_eq!(decode(0x1E, &mut s), Some(b'A'));
        assert_eq!(decode(0x02, &mut s), Some(b'!'));
        assert_eq!(decode(0xAA, &mut s), None);
        assert_eq!(decode(0x02, &mut s), Some(b'1'));
        assert_eq!(decode(0x36, &mut s), None);
        assert!(s.shift);
        assert_eq!(decode(0xB6, &mut s), None);
        assert!(!s.shift);
    }

    #[test]
    fn special_keys() {
        let mut s = KbdState::default();
        assert_eq!(decode(0x39, &mut s), Some(b' '));
        assert_eq!(decode(0x1C, &mut s), Some(b'\n'));
        assert_eq!(decode(0x0E, &mut s), Some(0x08));
        assert_eq!(decode(0x0F, &mut s), Some(b'\t'));
        assert_eq!(decode(0x35, &mut s), Some(b'/'));
    }

    #[test]
    fn releases_and_unknown_ignored() {
        let mut s = KbdState::default();
        assert_eq!(decode(0x9E, &mut s), None);
        assert_eq!(decode(0x01, &mut s), None);
        assert_eq!(decode(0x1D, &mut s), None);
        assert_eq!(decode(0x7F, &mut s), None);
        assert_eq!(decode(0xE0, &mut s), None);
        for sc in 0..=255u8 {
            let _ = decode(sc, &mut s);
        }
    }

    #[test]
    fn ring_fifo_and_empty() {
        let mut r = RingBuffer::new();
        assert!(r.is_empty());
        assert_eq!(r.pop(), None);
        r.push(1);
        r.push(2);
        assert!(!r.is_empty());
        assert_eq!(r.pop(), Some(1));
        assert_eq!(r.pop(), Some(2));
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn ring_full_drops_newest_and_wraps() {
        let mut r = RingBuffer::new();
        for i in 0..RingBuffer::CAP {
            assert!(r.push(i as u8));
        }
        assert!(!r.push(99));
        assert_eq!(r.pop(), Some(0));
        assert!(r.push(7));
        for i in 1..RingBuffer::CAP {
            assert_eq!(r.pop(), Some(i as u8));
        }
        assert_eq!(r.pop(), Some(7));
        assert_eq!(r.pop(), None);
    }

    fn ev(vk: u16, ch: u32, down: bool) -> KeyEvent {
        KeyEvent { vk, ch, down }
    }

    #[test]
    fn event_letter_down_up() {
        let mut s = KbdState::default();
        assert_eq!(
            decode_event(0x1E, &mut s),
            Some(ev(0x41, b'a' as u32, true))
        );
        assert_eq!(decode_event(0x9E, &mut s), Some(ev(0x41, 0, false)));
        assert_eq!(
            decode_event(0x02, &mut s),
            Some(ev(0x31, b'1' as u32, true))
        );
    }

    #[test]
    fn event_shift_uppercases() {
        let mut s = KbdState::default();
        assert_eq!(decode_event(0x2A, &mut s), Some(ev(VK_SHIFT, 0, true)));
        assert_eq!(
            decode_event(0x1E, &mut s),
            Some(ev(0x41, b'A' as u32, true))
        );
        assert_eq!(decode_event(0xAA, &mut s), Some(ev(VK_SHIFT, 0, false)));
        assert_eq!(
            decode_event(0x1E, &mut s),
            Some(ev(0x41, b'a' as u32, true))
        );
    }

    #[test]
    fn event_special_keys() {
        let mut s = KbdState::default();
        assert_eq!(decode_event(0x1C, &mut s), Some(ev(VK_RETURN, 0x0D, true)));
        assert_eq!(decode_event(0x0E, &mut s), Some(ev(VK_BACK, 0x08, true)));
        assert_eq!(decode_event(0x01, &mut s), Some(ev(VK_ESCAPE, 0, true)));
        assert_eq!(decode_event(0x39, &mut s), Some(ev(VK_SPACE, 0x20, true)));
        assert_eq!(decode_event(0x0F, &mut s), Some(ev(VK_TAB, 0x09, true)));
        assert_eq!(decode_event(0x81, &mut s), Some(ev(VK_ESCAPE, 0, false)));
    }

    #[test]
    fn event_unknown_never_panics() {
        let mut s = KbdState::default();
        assert_eq!(decode_event(0xE0, &mut s), None);
        assert_eq!(decode_event(0x1D, &mut s), None);
        for sc in 0..=255u8 {
            let _ = decode_event(sc, &mut s);
        }
    }

    fn feed3(d: &mut MouseDecoder, b: [u8; 3]) -> Option<MousePacket> {
        d.feed(b[0]);
        d.feed(b[1]);
        d.feed(b[2])
    }

    #[test]
    fn mouse_move_plus_x_minus_y() {
        let mut d = MouseDecoder::new();
        // byte0: bit3 + Y sign (0x20); dx=+5, dy=-3 (0xFD)
        let p = feed3(&mut d, [0x28, 5, 0xFD]).unwrap();
        assert_eq!((p.dx, p.dy), (5, -3));
        assert!(!p.left && !p.right && !p.middle);
    }

    #[test]
    fn mouse_negative_x_and_buttons() {
        let mut d = MouseDecoder::new();
        let p = feed3(&mut d, [0x18 | 1, 0xF6, 4]).unwrap();
        assert_eq!((p.dx, p.dy), (-10, 4));
        assert!(p.left && !p.right && !p.middle);
        assert!(feed3(&mut d, [0x08 | 2, 0, 0]).unwrap().right);
        assert!(feed3(&mut d, [0x08 | 4, 0, 0]).unwrap().middle);
    }

    #[test]
    fn mouse_overflow_saturates() {
        let mut d = MouseDecoder::new();
        let p = feed3(&mut d, [0x08 | 0x40 | 0x80, 0x10, 0x10]).unwrap();
        assert_eq!((p.dx, p.dy), (255, 255));
        let p = feed3(&mut d, [0x08 | 0x40 | 0x80 | 0x10 | 0x20, 0x10, 0x10]).unwrap();
        assert_eq!((p.dx, p.dy), (-255, -255));
    }

    #[test]
    fn mouse_resync_on_bad_byte0() {
        let mut d = MouseDecoder::new();
        assert_eq!(d.feed(0x00), None); // bit3==0: descartado
        assert_eq!(d.feed(0x07), None);
        let p = feed3(&mut d, [0x09, 1, 2]).unwrap();
        assert_eq!((p.dx, p.dy, p.left), (1, 2, true));
        for b in 0..=255u8 {
            let _ = d.feed(b);
        }
    }

    #[test]
    fn mouse_state_clamp_and_events() {
        let mut s = MouseState::new();
        let pk = |dx, dy, l| MousePacket {
            dx,
            dy,
            left: l,
            right: false,
            middle: false,
        };
        // y -= dy: dy negativo (mouse para baixo) aumenta y
        let ev = s.apply(pk(10, -20, false), 100, 50);
        assert_eq!((s.x, s.y), (10, 20));
        assert_eq!(
            ev[0],
            Some(InputEvent::Mouse {
                x: 10,
                y: 20,
                buttons: 0,
                kind: MouseKind::Move
            })
        );
        assert_eq!(ev[1], None);
        let ev = s.apply(pk(0, 0, true), 100, 50);
        assert_eq!(
            ev[0],
            Some(InputEvent::Mouse {
                x: 10,
                y: 20,
                buttons: 1,
                kind: MouseKind::Down(MouseButton::Left)
            })
        );
        let ev = s.apply(pk(500, -500, false), 100, 50);
        assert_eq!((s.x, s.y), (99, 49));
        assert!(matches!(
            ev[1],
            Some(InputEvent::Mouse {
                kind: MouseKind::Up(MouseButton::Left),
                ..
            })
        ));
        s.apply(pk(-500, 500, false), 100, 50);
        assert_eq!((s.x, s.y), (0, 0));
    }

    #[test]
    fn input_ring_mixed_and_pop_key() {
        let mut r = InputRing::new();
        let m = InputEvent::Mouse {
            x: 1,
            y: 2,
            buttons: 0,
            kind: MouseKind::Move,
        };
        r.push(m);
        r.push(ev(0x41, 0, true).into());
        assert_eq!(r.pop_key(), Some(ev(0x41, 0, true)));
        assert_eq!(r.pop_key(), None);
        for _ in 0..(InputRing::CAP + 5) {
            r.push(m);
        }
        assert_eq!(r.len(), InputRing::CAP);
    }

    #[test]
    fn event_ring_drops_oldest() {
        let mut r = EventRing::new();
        assert_eq!(r.pop(), None);
        for i in 0..(EventRing::CAP as u16 + 10) {
            r.push(ev(i, 0, true));
        }
        assert_eq!(r.len(), EventRing::CAP);
        assert_eq!(r.pop(), Some(ev(10, 0, true)));
    }
}
