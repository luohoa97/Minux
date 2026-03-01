#![no_std]
#![no_main]

use libminux::{syscall, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const FALLBACK_FS_ID: TaskId = 4;
const FALLBACK_TTY_ID: TaskId = 5;
const MAX_DEPTH: usize = 4;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let fs_id = discover_fs_service().unwrap_or(FALLBACK_FS_ID);
    let tty_id = discover_tty_service().unwrap_or(FALLBACK_TTY_ID);

    let mut buf = [0u8; 128];
    let args = read_args(&mut buf);
    let path = if args.is_empty() { b"/" } else { args };

    let mut buf = [0u8; 196];
    walk(fs_id, tty_id, path, 0, &mut buf);
    syscall::exit(0);
}

fn walk(fs_id: TaskId, tty_id: TaskId, path: &[u8], depth: usize, buf: &mut [u8; 196]) {
    if depth >= MAX_DEPTH {
        return;
    }
    let n = list_dir(fs_id, path, buf);
    if n == 0 {
        return;
    }
    let mut start = 0usize;
    for i in 0..=n {
        if i == n || buf[i] == b'\n' {
            if i > start {
                let mut name = [0u8; 64];
                let copy = core::cmp::min(i - start, name.len());
                name[..copy].copy_from_slice(&buf[start..start + copy]);
                if &name[..copy] == b"." || &name[..copy] == b".." {
                    start = i + 1;
                    continue;
                }
                print_entry(tty_id, depth, &name[..copy]);
                let mut next = [0u8; 96];
                let mut k = 0usize;
                k += write_ascii(&mut next[k..], path);
                if path != b"/" {
                    k += write_ascii(&mut next[k..], b"/");
                }
                k += write_ascii(&mut next[k..], &name[..copy]);
                let mut probe = [0u8; 196];
                if list_dir(fs_id, &next[..k], &mut probe) > 0 {
                    walk(fs_id, tty_id, &next[..k], depth + 1, buf);
                }
            }
            start = i + 1;
        }
    }
}

fn list_dir(fs_id: TaskId, path: &[u8], out: &mut [u8]) -> usize {
    let mut req = [0u8; 196];
    let mut n = 0usize;
    n += write_ascii(&mut req[n..], b"list:");
    n += write_ascii(&mut req[n..], path);

    let _ = syscall::send_message(fs_id, MessageType::Request, &req[..n]);
    for _ in 0..64 {
        if let Ok((sender, MessageType::Reply)) = syscall::receive_message(out) {
            if sender != fs_id {
                continue;
            }
            let len = out.iter().position(|&b| b == 0).unwrap_or(out.len());
            return len;
        }
        syscall::yield_cpu();
    }
    0
}

fn print_entry(tty_id: TaskId, depth: usize, name: &[u8]) {
    let mut line = [0u8; 96];
    let mut n = 0usize;
    for _ in 0..depth {
        n += write_ascii(&mut line[n..], b"  ");
    }
    n += write_ascii(&mut line[n..], b"|- ");
    n += write_ascii(&mut line[n..], name);
    n += write_ascii(&mut line[n..], b"\n");
    let _ = syscall::send_message(tty_id, MessageType::Notification, &line[..n]);
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
