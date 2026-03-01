#![no_std]
#![no_main]

use libminux::{syscall, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const FALLBACK_FS_ID: TaskId = 4;
const FALLBACK_TTY_ID: TaskId = 5;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let fs_id = discover_fs_service().unwrap_or(FALLBACK_FS_ID);
    let tty_id = discover_tty_service().unwrap_or(FALLBACK_TTY_ID);

    let mut buf = [0u8; 128];
    let args = read_args(&mut buf);
    let path = if args.is_empty() { b"/" } else { args };

    let mut req = [0u8; 196];
    let mut n = 0usize;
    n += write_ascii(&mut req[n..], b"list:");
    n += write_ascii(&mut req[n..], path);

    let mut out = [0u8; 196];
    let n = request_reply(fs_id, &req[..n], &mut out).unwrap_or(0);
    let reply = if n == 0 { b"NOTFOUND" } else { &out[..n] };
    let _ = syscall::send_message(tty_id, MessageType::Notification, reply);
    let _ = syscall::send_message(tty_id, MessageType::Notification, b"\n");
    syscall::exit(0);
}

fn read_args(buf: &mut [u8]) -> &[u8] {
    if let Ok((_sender, MessageType::Request)) = syscall::receive_message(buf) {
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        let data = &buf[..len];
        if let Some(rest) = strip_prefix(data, b"args:") {
            return rest;
        }
    }
    &[]
}

fn request_reply(target: TaskId, req: &[u8], out: &mut [u8]) -> Option<usize> {
    let _ = syscall::send_message(target, MessageType::Request, req);
    for _ in 0..64 {
        if let Ok((sender, MessageType::Reply)) = syscall::receive_message(out) {
            if sender != target {
                continue;
            }
            let len = out.iter().position(|&b| b == 0).unwrap_or(out.len());
            return Some(len);
        }
        syscall::yield_cpu();
    }
    None
}

fn discover_fs_service() -> Option<TaskId> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"lookup:fs");
    let mut reply = [0u8; 16];
    for _ in 0..64 {
        if let Ok((sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
            if sender != INIT_TASK_ID {
                continue;
            }
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            return parse_u32_ascii(&reply[..len]);
        }
        syscall::yield_cpu();
    }
    None
}

fn discover_tty_service() -> Option<TaskId> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"lookup:tty");
    let mut reply = [0u8; 16];
    for _ in 0..64 {
        if let Ok((sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
            if sender != INIT_TASK_ID {
                continue;
            }
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            return parse_u32_ascii(&reply[..len]);
        }
        syscall::yield_cpu();
    }
    None
}

fn strip_prefix<'a>(data: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    if data.len() < p.len() || &data[..p.len()] != p { None } else { Some(&data[p.len()..]) }
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
