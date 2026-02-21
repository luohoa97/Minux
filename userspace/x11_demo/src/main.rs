//! X11 demo client.
//! Talks to x11_server using a tiny x11:* subset and animates a cursor-dot path.

#![no_std]
#![no_main]

use libminux::{syscall, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const FALLBACK_X11_ID: TaskId = 8;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let x11 = discover_service(b"lookup:x11").unwrap_or(FALLBACK_X11_ID);

    let _ = request(x11, b"x11:hello");
    let _ = request(x11, b"x11:show_cursor:1");

    let mut x: u32 = 2;
    let mut y: u32 = 2;
    let mut dx: i32 = 1;
    let mut dy: i32 = 0;
    let mut tick: u64 = 0;

    loop {
        tick = tick.wrapping_add(1);
        if tick % 2_000_000 != 0 {
            syscall::yield_cpu();
            continue;
        }

        if x >= 40 {
            dx = 0;
            dy = 1;
        } else if y >= 12 {
            dx = -1;
            dy = 0;
        } else if x <= 2 {
            dx = 0;
            dy = -1;
        } else if y <= 2 {
            dx = 1;
            dy = 0;
        }

        x = ((x as i32) + dx) as u32;
        y = ((y as i32) + dy) as u32;

        let mut cmd = [0u8; 40];
        let mut n = 0usize;
        n += write_ascii(&mut cmd[n..], b"x11:set_cursor:");
        n += write_u32_ascii(x, &mut cmd[n..]);
        n += write_ascii(&mut cmd[n..], b":");
        n += write_u32_ascii(y, &mut cmd[n..]);

        let _ = request(x11, &cmd[..n]);
        let _ = request(x11, b"x11:flush");
        syscall::yield_cpu();
    }
}

fn request(target: TaskId, msg: &[u8]) -> Option<[u8; 16]> {
    let _ = syscall::send_message(target, MessageType::Request, msg);
    let mut reply = [0u8; 16];
    if let Ok((_sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
        Some(reply)
    } else {
        None
    }
}

fn discover_service(query: &[u8]) -> Option<TaskId> {
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
