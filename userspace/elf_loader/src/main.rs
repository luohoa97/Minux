#![no_std]
#![no_main]

use libminux::{syscall, MessageType};

const INIT_TASK_ID: u32 = 2;
const FALLBACK_FS_TASK_ID: u32 = 4;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"register:loader:elf_loader");

    loop {
        let mut buf = [0u8; 128];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                let reply_to = if sender == 0 { INIT_TASK_ID } else { sender };
                if !matches!(msg_type, MessageType::Request) {
                    continue;
                }
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let data = &buf[..len];

                if let Some(name) = strip_prefix(data, b"exec:") {
                    match exec_path_or_module(name) {
                        Ok(task_id) => {
                            let mut out = [0u8; 16];
                            let n = write_u32_ascii(task_id, &mut out);
                            let _ = syscall::reply_message(reply_to, &out[..n]);
                        }
                        Err(_) => {
                            let _ = syscall::reply_message(reply_to, b"ERR");
                        }
                    }
                } else {
                    let _ = syscall::reply_message(reply_to, b"BADREQ");
                }
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
}

fn exec_path_or_module(target: &[u8]) -> Result<u32, ()> {
    if target.starts_with(b"/") {
        let fs = discover_fs().unwrap_or(FALLBACK_FS_TASK_ID);
        let mut req = [0u8; 96];
        req[..8].copy_from_slice(b"resolve:");
        if target.len() + 8 > req.len() {
            return Err(());
        }
        req[8..8 + target.len()].copy_from_slice(target);
        let _ = syscall::send_message(fs, MessageType::Request, &req[..8 + target.len()]);
        let mut reply = [0u8; 64];
        if let Ok((_sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            let name = &reply[..len];
            if name == b"NOTFOUND" || name.is_empty() {
                return Err(());
            }
            return syscall::exec_module(name);
        }
        Err(())
    } else {
        syscall::exec_module(target)
    }
}

fn discover_fs() -> Option<u32> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"lookup:vfs");
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

fn strip_prefix<'a>(data: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    if data.len() < p.len() || &data[..p.len()] != p { None } else { Some(&data[p.len()..]) }
}

fn write_u32_ascii(id: u32, out: &mut [u8]) -> usize {
    if out.is_empty() { return 0; }
    if id == 0 { out[0] = b'0'; return 1; }
    let mut tmp = [0u8; 10];
    let mut n = id;
    let mut k = 0usize;
    while n > 0 && k < tmp.len() {
        tmp[k] = b'0' + (n % 10) as u8;
        n /= 10;
        k += 1;
    }
    let len = core::cmp::min(k, out.len());
    for j in 0..len { out[j] = tmp[k - 1 - j]; }
    len
}
