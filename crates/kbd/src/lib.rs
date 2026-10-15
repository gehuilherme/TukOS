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
}
