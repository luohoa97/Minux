//! Input service.
//! Owns keyboard chord policy in userspace (Ctrl+Alt+Fn -> tty:switch:N).

#![no_std]
#![no_main]

use libminux::{syscall, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const FALLBACK_GFX_ID: TaskId = 3;
const FALLBACK_CONSOLE_ID: TaskId = 5;

#[derive(Clone, Copy)]
struct KbdState {
    ctrl_down: bool,
    alt_down: bool,
    shift_down: bool,
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let gfx_id = wait_for_service(b"lookup:gfx").unwrap_or(FALLBACK_GFX_ID);
    let console_id = wait_for_service(b"lookup:tty").unwrap_or(FALLBACK_CONSOLE_ID);

    let _ = syscall::send_message(gfx_id, MessageType::Request, b"\n[INPUT] service online");
    let _ = syscall::send_message(
        INIT_TASK_ID,
        MessageType::Request,
        b"register:input:input_service",
    );

    let mut state = KbdState {
        ctrl_down: false,
        alt_down: false,
        shift_down: false,
    };

    loop {
        if let Some(sc) = syscall::read_scancode() {
            handle_scancode(sc, &mut state, console_id, gfx_id);
        }

        let mut buf = [0u8; 128];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let data = &buf[..len];

                // Testing hook until keyboard driver is wired:
                // send "inject:<scancode_hex>" requests to this service.
                if let Some(sc) = parse_inject_hex(data) {
                    handle_scancode(sc, &mut state, console_id, gfx_id);
                    let _ = syscall::reply_message(sender, b"INPUT:OK");
                    continue;
                }

                // Future keyboard driver path: 1-byte raw Set1 scancode in notification/interrupt.
                if matches!(msg_type, MessageType::Notification | MessageType::Interrupt) && data.len() == 1 {
                    handle_scancode(data[0], &mut state, console_id, gfx_id);
                    continue;
                }

                // Poll API for clients (placeholder): no queued events yet.
                if matches!(msg_type, MessageType::Request) {
                    let _ = syscall::reply_message(sender, b"INPUT:NONE");
                }
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
}

fn handle_scancode(sc: u8, state: &mut KbdState, console_id: TaskId, gfx_id: TaskId) {
    // Set1 scancodes: Ctrl=0x1D, Alt=0x38, Shift=0x2A/0x36, F1..F6 = 0x3B..0x40
    let is_break = (sc & 0x80) != 0;
    let code = sc & 0x7F;

    match code {
        0x1D => state.ctrl_down = !is_break,
        0x38 => state.alt_down = !is_break,
        0x2A | 0x36 => state.shift_down = !is_break,
        0x3B..=0x40 if !is_break => {
            if state.ctrl_down && state.alt_down {
                let tty_num = (code - 0x3B) + 1; // F1->tty1 ... F6->tty6
                let mut cmd = [0u8; 16];
                let mut n = 0usize;
                n += write_ascii(&mut cmd[n..], b"tty:switch:");
                n += write_u32_ascii(tty_num as u32, &mut cmd[n..]);
                let _ = syscall::send_message(console_id, MessageType::Request, &cmd[..n]);

                let mut log = [0u8; 40];
                let mut k = 0usize;
                k += write_ascii(&mut log[k..], b"\n[INPUT] Ctrl+Alt+F");
                k += write_u32_ascii(tty_num as u32, &mut log[k..]);
                k += write_ascii(&mut log[k..], b" -> tty switch");
                let _ = syscall::send_message(gfx_id, MessageType::Request, &log[..k]);
            }
        }
        _ if !is_break => {
            if let Some(ch) = scancode_to_ascii(code, state.shift_down) {
                let msg = [b'k', b'e', b'y', b':', ch];
                let _ = syscall::send_message(console_id, MessageType::Notification, &msg);
            }
        }
        _ => {}
    }
}

fn scancode_to_ascii(code: u8, shift: bool) -> Option<u8> {
    let c = match code {
        0x02 => if shift { b'!' } else { b'1' },
        0x03 => if shift { b'@' } else { b'2' },
        0x04 => if shift { b'#' } else { b'3' },
        0x05 => if shift { b'$' } else { b'4' },
        0x06 => if shift { b'%' } else { b'5' },
        0x07 => if shift { b'^' } else { b'6' },
        0x08 => if shift { b'&' } else { b'7' },
        0x09 => if shift { b'*' } else { b'8' },
        0x0A => if shift { b'(' } else { b'9' },
        0x0B => if shift { b')' } else { b'0' },
        0x0C => if shift { b'_' } else { b'-' },
        0x0D => if shift { b'+' } else { b'=' },
        0x0E => 0x08, // backspace
        0x0F => b'\t',
        0x10 => if shift { b'Q' } else { b'q' },
        0x11 => if shift { b'W' } else { b'w' },
        0x12 => if shift { b'E' } else { b'e' },
        0x13 => if shift { b'R' } else { b'r' },
        0x14 => if shift { b'T' } else { b't' },
        0x15 => if shift { b'Y' } else { b'y' },
        0x16 => if shift { b'U' } else { b'u' },
        0x17 => if shift { b'I' } else { b'i' },
        0x18 => if shift { b'O' } else { b'o' },
        0x19 => if shift { b'P' } else { b'p' },
        0x1A => if shift { b'{' } else { b'[' },
        0x1B => if shift { b'}' } else { b']' },
        0x1C => b'\n',
        0x1E => if shift { b'A' } else { b'a' },
        0x1F => if shift { b'S' } else { b's' },
        0x20 => if shift { b'D' } else { b'd' },
        0x21 => if shift { b'F' } else { b'f' },
        0x22 => if shift { b'G' } else { b'g' },
        0x23 => if shift { b'H' } else { b'h' },
        0x24 => if shift { b'J' } else { b'j' },
        0x25 => if shift { b'K' } else { b'k' },
        0x26 => if shift { b'L' } else { b'l' },
        0x27 => if shift { b':' } else { b';' },
        0x28 => if shift { b'"' } else { b'\'' },
        0x29 => if shift { b'~' } else { b'`' },
        0x2B => if shift { b'|' } else { b'\\' },
        0x2C => if shift { b'Z' } else { b'z' },
        0x2D => if shift { b'X' } else { b'x' },
        0x2E => if shift { b'C' } else { b'c' },
        0x2F => if shift { b'V' } else { b'v' },
        0x30 => if shift { b'B' } else { b'b' },
        0x31 => if shift { b'N' } else { b'n' },
        0x32 => if shift { b'M' } else { b'm' },
        0x33 => if shift { b'<' } else { b',' },
        0x34 => if shift { b'>' } else { b'.' },
        0x35 => if shift { b'?' } else { b'/' },
        0x39 => b' ',
        _ => return None,
    };
    Some(c)
}

fn discover_service(query: &[u8]) -> Option<TaskId> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, query);
    let mut reply = [0u8; 16];
    if let Ok((sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
        if sender != INIT_TASK_ID {
            return None;
        }
        let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
        let id = parse_u32_ascii(&reply[..len])?;
        if id == INIT_TASK_ID {
            return None;
        }
        return Some(id);
    }
    None
}

fn wait_for_service(query: &[u8]) -> Option<TaskId> {
    for _ in 0..1_000_000u32 {
        if let Some(id) = discover_service(query) {
            return Some(id);
        }
        syscall::yield_cpu();
    }
    None
}

fn parse_inject_hex(data: &[u8]) -> Option<u8> {
    const P: &[u8] = b"inject:";
    if data.len() < P.len() || &data[..P.len()] != P {
        return None;
    }
    parse_hex_u8(&data[P.len()..])
}

fn parse_hex_u8(data: &[u8]) -> Option<u8> {
    if data.is_empty() || data.len() > 2 {
        return None;
    }
    let mut v: u8 = 0;
    for &b in data {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?;
        v = v.checked_add(d)?;
    }
    Some(v)
}

fn parse_u32_ascii(data: &[u8]) -> Option<u32> {
    if data.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    for &b in data {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?;
        v = v.checked_add((b - b'0') as u32)?;
    }
    if v == 0 { None } else { Some(v) }
}

fn write_ascii(out: &mut [u8], s: &[u8]) -> usize {
    let n = core::cmp::min(out.len(), s.len());
    out[..n].copy_from_slice(&s[..n]);
    n
}

fn write_u32_ascii(id: u32, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    if id == 0 {
        out[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 10];
    let mut n = id;
    let mut k = 0usize;
    while n > 0 && k < tmp.len() {
        tmp[k] = b'0' + (n % 10) as u8;
        n /= 10;
        k += 1;
    }
    let len = core::cmp::min(k, out.len());
    for j in 0..len {
        out[j] = tmp[k - 1 - j];
    }
    len
}
