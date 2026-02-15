use std::io::{self, Read};

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Delete,
    Ctrl(char),
    MouseClick { row: u16, col: u16 },
    ScrollUp,
    ScrollDown,
}

/// Read a single keypress from stdin.
/// Returns None if no key is available (timeout expired).
pub fn read_key() -> Option<Key> {
    let mut buf = [0u8; 1];
    match io::stdin().read(&mut buf) {
        Ok(0) => None, // timeout, no data
        Ok(_) => Some(parse_byte(buf[0])),
        Err(_) => None,
    }
}

fn parse_byte(b: u8) -> Key {
    match b {
        13 => Key::Enter,
        27 => parse_escape(),
        127 => Key::Backspace,
        9 => Key::Tab,
        b @ 1..=26 => Key::Ctrl((b'a' + b - 1) as char),
        b if (32..127).contains(&b) => Key::Char(b as char),
        _ => Key::Char('?'),
    }
}

fn parse_escape() -> Key {
    // Try to read the next byte quickly
    let mut buf = [0u8; 1];
    match io::stdin().read(&mut buf) {
        Ok(0) => return Key::Escape, // bare escape
        Ok(_) => {}
        Err(_) => return Key::Escape,
    }

    if buf[0] != b'[' {
        return Key::Escape; // not a CSI sequence
    }

    // Read the sequence character
    match io::stdin().read(&mut buf) {
        Ok(0) => return Key::Escape,
        Ok(_) => {}
        Err(_) => return Key::Escape,
    }

    match buf[0] {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        // Extended sequences like ESC [ 5 ~
        b'1'..=b'8' => {
            let num = buf[0];
            // Read the trailing ~ or ;
            match io::stdin().read(&mut buf) {
                Ok(0) => Key::Escape,
                Ok(_) => {
                    if buf[0] == b'~' {
                        match num {
                            b'3' => Key::Delete,
                            b'5' => Key::PageUp,
                            b'6' => Key::PageDown,
                            _ => Key::Escape,
                        }
                    } else {
                        // Consume any remaining bytes of the sequence
                        Key::Escape
                    }
                }
                Err(_) => Key::Escape,
            }
        }
        // SGR mouse: ESC [ < ...
        b'<' => parse_sgr_mouse(),
        _ => Key::Escape,
    }
}

fn parse_sgr_mouse() -> Key {
    // SGR format: ESC [ < btn ; col ; row M (press) or m (release)
    let mut params = [0u16; 3];
    let mut param_idx = 0;
    let mut buf = [0u8; 1];

    loop {
        match io::stdin().read(&mut buf) {
            Ok(0) | Err(_) => return Key::Escape,
            Ok(_) => {}
        }
        match buf[0] {
            b'0'..=b'9' => {
                if param_idx < 3 {
                    params[param_idx] = params[param_idx]
                        .saturating_mul(10)
                        .saturating_add((buf[0] - b'0') as u16);
                }
            }
            b';' => {
                param_idx += 1;
                if param_idx >= 3 {
                    // Too many params, consume until terminator
                    loop {
                        match io::stdin().read(&mut buf) {
                            Ok(0) | Err(_) => return Key::Escape,
                            Ok(_) if buf[0] == b'M' || buf[0] == b'm' => return Key::Escape,
                            _ => {}
                        }
                    }
                }
            }
            b'M' => {
                if param_idx != 2 {
                    return Key::Escape;
                }
                return match params[0] {
                    0 => Key::MouseClick {
                        row: params[2],
                        col: params[1],
                    },
                    64 => Key::ScrollUp,
                    65 => Key::ScrollDown,
                    _ => Key::Escape,
                };
            }
            b'm' => {
                // Release event — ignore
                return Key::Escape;
            }
            _ => return Key::Escape,
        }
    }
}
