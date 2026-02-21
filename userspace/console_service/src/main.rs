//! Console (TTY) service.
//! Provides a stable text console endpoint for apps/shell, forwarding rendering to gfx service.

#![no_std]
#![no_main]

use libminux::{syscall, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const FALLBACK_GFX_ID: TaskId = 3;
const NUM_TTYS: usize = 6;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = syscall::send_message(
        INIT_TASK_ID,
        MessageType::Request,
        b"register:tty:console_service",
    );

    let gfx_id = wait_for_service(b"lookup:gfx").unwrap_or(FALLBACK_GFX_ID);
    let input_id = wait_for_service(b"lookup:input").unwrap_or(INIT_TASK_ID);

    let _ = syscall::send_message(gfx_id, MessageType::Notification, b"clear");
    let _ = syscall::send_message(gfx_id, MessageType::Request, b"\n[CONSOLE] tty online");
    let _ = syscall::send_message(gfx_id, MessageType::Request, b"\nWelcome to Minux");
    let mut current_tty: usize = 0;
    let mut foreground: [Option<TaskId>; NUM_TTYS] = [None; NUM_TTYS];

    loop {
        let mut buf = [0u8; 160];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let data = &buf[..len];

                if data == b"tty:acquire" {
                    if foreground[current_tty].is_none() || foreground[current_tty] == Some(sender) {
                        foreground[current_tty] = Some(sender);
                        let _ = syscall::reply_message(sender, b"TTY:FG");
                        let _ = syscall::send_message(gfx_id, MessageType::Request, b"\n[CONSOLE] foreground granted");
                    } else {
                        let _ = syscall::reply_message(sender, b"TTY:BUSY");
                    }
                    continue;
                }

                if data == b"tty:release" {
                    if foreground[current_tty] == Some(sender) {
                        foreground[current_tty] = None;
                    }
                    let _ = syscall::reply_message(sender, b"TTY:REL");
                    continue;
                }

                if data.len() == 5 && &data[..4] == b"key:" {
                    if let Some(owner) = foreground[current_tty] {
                        let _ = syscall::send_message(owner, MessageType::Notification, data);
                    }
                    continue;
                }

                if sender == input_id {
                    if let Some(next_tty) = parse_tty_switch(data) {
                        if next_tty < NUM_TTYS {
                            current_tty = next_tty;
                            let _ = syscall::send_message(gfx_id, MessageType::Notification, b"clear");
                            let mut msg = [0u8; 48];
                            let mut n = 0usize;
                            n += write_ascii(&mut msg[n..], b"\n[CONSOLE] switched to tty");
                            n += write_u32_ascii((current_tty + 1) as u32, &mut msg[n..]);
                            let _ = syscall::send_message(gfx_id, MessageType::Request, &msg[..n]);
                            let _ = syscall::reply_message(sender, b"TTY:SWITCHED");
                        } else {
                            let _ = syscall::reply_message(sender, b"TTY:BAD");
                        }
                        continue;
                    }
                }

                if let Some(owner) = foreground[current_tty] {
                    if owner != sender {
                        if matches!(msg_type, MessageType::Request) {
                            let _ = syscall::reply_message(sender, b"TTY:BUSY");
                        }
                        continue;
                    }
                }

                match msg_type {
                    MessageType::Notification => {
                        let _ = syscall::send_message(gfx_id, MessageType::Notification, data);
                    }
                    MessageType::Request => {
                        let _ = syscall::send_message(gfx_id, MessageType::Request, data);
                        let _ = syscall::reply_message(sender, b"TTY:OK");
                    }
                    _ => {}
                }
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
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
    loop {
        if let Some(id) = discover_service(query) {
            return Some(id);
        }
        syscall::yield_cpu();
    }
}

fn parse_tty_switch(data: &[u8]) -> Option<usize> {
    const P: &[u8] = b"tty:switch:";
    if data.len() < P.len() || &data[..P.len()] != P {
        return None;
    }
    let n = parse_u32_ascii(&data[P.len()..])?;
    if n == 0 {
        return None;
    }
    Some((n - 1) as usize)
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
