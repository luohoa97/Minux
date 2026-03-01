//! Input service.
//! Owns keyboard chord policy in userspace (Ctrl+Alt+Fn -> tty:switch:N).

#![no_std]
#![no_main]

use libminux::{syscall, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const FALLBACK_GFX_ID: TaskId = 3;
const FALLBACK_CONSOLE_ID: TaskId = 5;
const INPUT_DEBUG: bool = false;

#[derive(Clone, Copy)]
struct KbdState {
    ctrl_down: bool,
    alt_down: bool,
    shift_down: bool,
    e0_prefix: bool,
    f0_prefix: bool,
    set2_mode: bool,
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let gfx_id = wait_for_service(b"lookup:gfx").unwrap_or(FALLBACK_GFX_ID);
    let mut console_id = discover_service(b"lookup:tty").unwrap_or(FALLBACK_CONSOLE_ID);

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
        e0_prefix: false,
        f0_prefix: false,
        set2_mode: false,
    };
    let mut probe_tick: u32 = 0;
    let mut dbg_sc_count: u32 = 0;

    loop {
        // Console service can come up after input_service; refresh target lazily.
        if (probe_tick & 0x3ff) == 0 {
            if let Some(id) = discover_service(b"lookup:tty") {
                console_id = id;
            }
            if INPUT_DEBUG && (probe_tick & 0x3fff) == 0 {
                let mut msg = [0u8; 40];
                let mut k = 0usize;
                k += write_ascii(&mut msg[k..], b"\n[INDBG] alive tty=");
                k += write_u32_ascii(console_id, &mut msg[k..]);
                let _ = syscall::send_message(gfx_id, MessageType::Notification, &msg[..k]);
            }
        }
        probe_tick = probe_tick.wrapping_add(1);

        if let Some(sc) = syscall::read_scancode() {
            dbg_sc_count = dbg_sc_count.wrapping_add(1);
            if INPUT_DEBUG {
                log_scancode(gfx_id, sc, dbg_sc_count);
            }
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
    // PS/2 Set 2 break prefix
    if sc == 0xF0 {
        state.set2_mode = true;
        state.f0_prefix = true;
        return;
    }

    if sc == 0xE0 {
        state.e0_prefix = true;
        return;
    }

    // Default to Set 1 (observed on this platform). Switch to Set 2 only after 0xF0.
    let set1_break = (sc & 0x80) != 0;
    if set1_break {
        state.set2_mode = false;
    }
    let code = sc & 0x7F;
    let is_break = if state.set2_mode { state.f0_prefix } else { set1_break };

    if state.e0_prefix {
        state.e0_prefix = false;
        state.f0_prefix = false;
        if !is_break {
            match sc {
                // Set 1 extended arrows
                0x48 => send_ansi_key(console_id, b'A'), // Up
                0x50 => send_ansi_key(console_id, b'B'), // Down
                0x4D => send_ansi_key(console_id, b'C'), // Right
                0x4B => send_ansi_key(console_id, b'D'), // Left
                // Set 2 extended arrows (only if Set 2 detected)
                0x75 if state.set2_mode => send_ansi_key(console_id, b'A'),
                0x72 if state.set2_mode => send_ansi_key(console_id, b'B'),
                0x74 if state.set2_mode => send_ansi_key(console_id, b'C'),
                0x6B if state.set2_mode => send_ansi_key(console_id, b'D'),
                _ => {}
            }
        }
        return;
    }

    // Set 2 modifiers/Fn (make codes) only when Set 2 is active.
    if state.set2_mode {
        match sc {
            0x14 => {
                state.ctrl_down = !is_break;
                state.f0_prefix = false;
                return;
            }
            0x11 => {
                state.alt_down = !is_break;
                state.f0_prefix = false;
                return;
            }
            0x12 | 0x59 => {
                state.shift_down = !is_break;
                state.f0_prefix = false;
                return;
            }
            _ => {}
        }
    }
    if state.set2_mode && !is_break && state.ctrl_down && state.alt_down {
        let tty_num = match sc {
            0x05 => Some(1), // F1
            0x06 => Some(2), // F2
            0x04 => Some(3), // F3
            0x0C => Some(4), // F4
            0x03 => Some(5), // F5
            0x0B => Some(6), // F6
            _ => None,
        };
        if let Some(tty_num) = tty_num {
            let mut cmd = [0u8; 16];
            let mut n = 0usize;
            n += write_ascii(&mut cmd[n..], b"tty:switch:");
            n += write_u32_ascii(tty_num as u32, &mut cmd[n..]);
            send_with_retry(console_id, MessageType::Request as u32, &cmd[..n]);

            let mut log = [0u8; 40];
            let mut k = 0usize;
            k += write_ascii(&mut log[k..], b"\n[INPUT] Ctrl+Alt+F");
            k += write_u32_ascii(tty_num as u32, &mut log[k..]);
            k += write_ascii(&mut log[k..], b" -> tty switch");
            send_with_retry(gfx_id, MessageType::Request as u32, &log[..k]);
            state.f0_prefix = false;
            return;
        }
    }

    if !is_break {
        if state.ctrl_down {
            let ctrl = match sc {
                // Set 2
                0x21 => Some(0x03), // Ctrl+C
                0x23 => Some(0x04), // Ctrl+D
                // Set 1 (mask-bits form also below, but explicit here first)
                0x2E => Some(0x03),
                0x20 => Some(0x04),
                _ => None,
            };
            if let Some(v) = ctrl {
                if !send_with_retry(console_id, MessageType::Notification as u32, &[v]) && INPUT_DEBUG {
                    log_send_fail(gfx_id, b"ctrl");
                }
                state.f0_prefix = false;
                return;
            }
        }
        if state.set2_mode {
            if let Some(ch) = scancode_set2_to_ascii(sc, state.shift_down) {
            let msg = [ch];
            if !send_with_retry(console_id, MessageType::Notification as u32, &msg) && INPUT_DEBUG {
                log_send_fail(gfx_id, b"s2");
            }
            state.f0_prefix = false;
            return;
            }
        }
    }

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
                send_with_retry(console_id, MessageType::Request as u32, &cmd[..n]);

                let mut log = [0u8; 40];
                let mut k = 0usize;
                k += write_ascii(&mut log[k..], b"\n[INPUT] Ctrl+Alt+F");
                k += write_u32_ascii(tty_num as u32, &mut log[k..]);
                k += write_ascii(&mut log[k..], b" -> tty switch");
                send_with_retry(gfx_id, MessageType::Request as u32, &log[..k]);
            }
        }
        _ if !is_break => {
            if state.ctrl_down {
                let ctrl = match code {
                    0x2E => Some(0x03), // Ctrl+C
                    0x20 => Some(0x04), // Ctrl+D
                    _ => None,
                };
                if let Some(v) = ctrl {
                    if !send_with_retry(console_id, MessageType::Notification as u32, &[v]) && INPUT_DEBUG {
                        log_send_fail(gfx_id, b"ctrl");
                    }
                    state.f0_prefix = false;
                    return;
                }
            }
            if let Some(ch) = scancode_to_ascii(code, state.shift_down) {
                let msg = [ch];
                if !send_with_retry(console_id, MessageType::Notification as u32, &msg) && INPUT_DEBUG {
                    log_send_fail(gfx_id, b"s1");
                }
            }
        }
        _ => {}
    }
    state.f0_prefix = false;
}

fn send_ansi_key(console_id: TaskId, code: u8) {
    send_with_retry(console_id, MessageType::Notification as u32, &[0x1B]);
    send_with_retry(console_id, MessageType::Notification as u32, b"[");
    send_with_retry(console_id, MessageType::Notification as u32, &[code]);
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

fn scancode_set2_to_ascii(code: u8, shift: bool) -> Option<u8> {
    let c = match code {
        0x16 => if shift { b'!' } else { b'1' },
        0x1E => if shift { b'@' } else { b'2' },
        0x26 => if shift { b'#' } else { b'3' },
        0x25 => if shift { b'$' } else { b'4' },
        0x2E => if shift { b'%' } else { b'5' },
        0x36 => if shift { b'^' } else { b'6' },
        0x3D => if shift { b'&' } else { b'7' },
        0x3E => if shift { b'*' } else { b'8' },
        0x46 => if shift { b'(' } else { b'9' },
        0x45 => if shift { b')' } else { b'0' },
        0x4E => if shift { b'_' } else { b'-' },
        0x55 => if shift { b'+' } else { b'=' },
        0x66 => 0x08, // backspace
        0x0D => b'\t',
        0x15 => if shift { b'Q' } else { b'q' },
        0x1D => if shift { b'W' } else { b'w' },
        0x24 => if shift { b'E' } else { b'e' },
        0x2D => if shift { b'R' } else { b'r' },
        0x2C => if shift { b'T' } else { b't' },
        0x35 => if shift { b'Y' } else { b'y' },
        0x3C => if shift { b'U' } else { b'u' },
        0x43 => if shift { b'I' } else { b'i' },
        0x44 => if shift { b'O' } else { b'o' },
        0x4D => if shift { b'P' } else { b'p' },
        0x54 => if shift { b'{' } else { b'[' },
        0x5B => if shift { b'}' } else { b']' },
        0x5A => b'\n',
        0x1C => if shift { b'A' } else { b'a' },
        0x1B => if shift { b'S' } else { b's' },
        0x23 => if shift { b'D' } else { b'd' },
        0x2B => if shift { b'F' } else { b'f' },
        0x34 => if shift { b'G' } else { b'g' },
        0x33 => if shift { b'H' } else { b'h' },
        0x3B => if shift { b'J' } else { b'j' },
        0x42 => if shift { b'K' } else { b'k' },
        0x4B => if shift { b'L' } else { b'l' },
        0x4C => if shift { b':' } else { b';' },
        0x52 => if shift { b'"' } else { b'\'' },
        0x0E => if shift { b'~' } else { b'`' },
        0x5D => if shift { b'|' } else { b'\\' },
        0x1A => if shift { b'Z' } else { b'z' },
        0x22 => if shift { b'X' } else { b'x' },
        0x21 => if shift { b'C' } else { b'c' },
        0x2A => if shift { b'V' } else { b'v' },
        0x32 => if shift { b'B' } else { b'b' },
        0x31 => if shift { b'N' } else { b'n' },
        0x3A => if shift { b'M' } else { b'm' },
        0x41 => if shift { b'<' } else { b',' },
        0x49 => if shift { b'>' } else { b'.' },
        0x4A => if shift { b'?' } else { b'/' },
        0x29 => b' ',
        _ => return None,
    };
    Some(c)
}

fn discover_service(query: &[u8]) -> Option<TaskId> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, query);
    // Init reply may arrive a bit later than this call site; poll briefly.
    for _ in 0..256u32 {
        let mut reply = [0u8; 16];
        if let Ok((sender, msg_type)) = syscall::receive_message(&mut reply) {
            if sender != INIT_TASK_ID || !matches!(msg_type, MessageType::Reply) {
                continue;
            }
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            let id = parse_u32_ascii(&reply[..len])?;
            if id == INIT_TASK_ID {
                return None;
            }
            return Some(id);
        }
        syscall::yield_cpu();
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

fn send_with_retry(target: TaskId, msg_type: u32, data: &[u8]) -> bool {
    for _ in 0..64 {
        let ty = match msg_type {
            0 => MessageType::Request,
            1 => MessageType::Reply,
            2 => MessageType::Notification,
            3 => MessageType::Interrupt,
            _ => MessageType::Notification,
        };
        if syscall::send_message(target, ty, data).is_ok() {
            return true;
        }
        syscall::yield_cpu();
    }
    false
}

fn log_scancode(gfx_id: TaskId, sc: u8, n: u32) {
    let mut msg = [0u8; 48];
    let mut k = 0usize;
    k += write_ascii(&mut msg[k..], b"\n[INDBG] sc=");
    k += write_hex_u8(sc, &mut msg[k..]);
    k += write_ascii(&mut msg[k..], b" n=");
    k += write_u32_ascii(n, &mut msg[k..]);
    let _ = syscall::send_message(gfx_id, MessageType::Notification, &msg[..k]);
}

fn log_send_fail(gfx_id: TaskId, tag: &[u8]) {
    let mut msg = [0u8; 48];
    let mut k = 0usize;
    k += write_ascii(&mut msg[k..], b"\n[INDBG] send fail ");
    k += write_ascii(&mut msg[k..], tag);
    let _ = syscall::send_message(gfx_id, MessageType::Notification, &msg[..k]);
}

fn write_hex_u8(v: u8, out: &mut [u8]) -> usize {
    if out.len() < 2 {
        return 0;
    }
    let hex = b"0123456789ABCDEF";
    out[0] = hex[(v >> 4) as usize];
    out[1] = hex[(v & 0x0F) as usize];
    2
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
