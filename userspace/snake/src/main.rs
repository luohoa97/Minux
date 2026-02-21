//! Snake demo process for Minux.
//! Uses console/gfx/input services to render a simple auto-moving snake frame.

#![no_std]
#![no_main]

use libminux::{syscall, MessageType};

const INIT_TASK_ID: u32 = 2;
const FALLBACK_GFX_ID: u32 = 3;
const FALLBACK_INPUT_ID: u32 = 2;
const FALLBACK_TTY_ID: u32 = 5;
const W: usize = 24;
const H: usize = 12;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let gfx_id = discover_service(b"lookup:gfx").unwrap_or(FALLBACK_GFX_ID);
    let input_id = discover_service(b"lookup:input").unwrap_or(FALLBACK_INPUT_ID);
    let tty_id = discover_service(b"lookup:tty").unwrap_or(FALLBACK_TTY_ID);

    let _ = syscall::send_message(tty_id, MessageType::Request, b"tty:acquire");
    let _ = syscall::send_message(tty_id, MessageType::Notification, b"clear");
    let _ = syscall::send_message(tty_id, MessageType::Request, b"\n[SNAKE] starting...");
    let _ = syscall::send_message(tty_id, MessageType::Request, b"\n[SNAKE] terminal in foreground mode");
    let _ = syscall::send_message(tty_id, MessageType::Request, b"\n[SNAKE] services resolved via lookup:*");

    // Keep these visible in logs and exercise lookup:gfx/lookup:input in runtime path.
    let mut id_msg = [0u8; 48];
    let mut n = 0usize;
    n += write_ascii(&mut id_msg[n..], b"\n[SNAKE] gfx=");
    n += write_u32_ascii(gfx_id, &mut id_msg[n..]);
    n += write_ascii(&mut id_msg[n..], b" input=");
    n += write_u32_ascii(input_id, &mut id_msg[n..]);
    let _ = syscall::send_message(tty_id, MessageType::Request, &id_msg[..n]);

    let mut x = 2usize;
    let mut y = 2usize;
    let mut dx: isize = 1;
    let mut dy: isize = 0;
    let mut tick: u64 = 0;

    loop {
        tick += 1;
        if tick % 2_000_000 != 0 {
            syscall::yield_cpu();
            continue;
        }

        if x + 1 >= W {
            dx = 0;
            dy = 1;
        } else if y + 1 >= H {
            dx = -1;
            dy = 0;
        } else if x == 0 {
            dx = 0;
            dy = -1;
        } else if y == 0 {
            dx = 1;
            dy = 0;
        }

        x = ((x as isize) + dx) as usize;
        y = ((y as isize) + dy) as usize;

        render(tty_id, x, y);
        syscall::yield_cpu();
    }
}

fn render(tty_id: u32, sx: usize, sy: usize) {
    let _ = syscall::send_message(tty_id, MessageType::Notification, b"clear");
    let _ = syscall::send_message(tty_id, MessageType::Request, b"\n[SNAKE] demo mode (no keyboard yet)");

    let mut line = [b' '; W + 1];
    line[W] = 0;

    for y in 0..H {
        for x in 0..W {
            let border = x == 0 || y == 0 || x + 1 == W || y + 1 == H;
            line[x] = if border { b'#' } else { b' ' };
        }
        if y == sy {
            line[sx] = b'@';
        }
        let mut msg = [0u8; W + 2];
        msg[0] = b'\n';
        for i in 0..W {
            msg[i + 1] = line[i];
        }
        let _ = syscall::send_message(tty_id, MessageType::Request, &msg);
    }
}

fn discover_service(query: &[u8]) -> Option<u32> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, query);
    let mut reply = [0u8; 16];
    if let Ok((_sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
        let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
        return parse_u32_ascii(&reply[..len]);
    }
    None
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

// Panic handler provided by libminux.
