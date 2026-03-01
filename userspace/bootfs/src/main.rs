#![no_std]
#![no_main]

use libminux::{syscall, MessageType};

const INIT_TASK_ID: u32 = 2;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"register:bootfs:bootfs_service");

    loop {
        let mut buf = [0u8; 128];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                if !matches!(msg_type, MessageType::Request) {
                    continue;
                }
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let data = &buf[..len];

                if let Some(path) = strip_prefix(data, b"list:") {
                    let mut out = [0u8; 196];
                    if let Some(n) = syscall::bootfs_list(&mut out) {
                        let list = &out[..n];
                        // Synthetic directory structure.
                        if path == b"/" {
                            let _ = syscall::reply_message(sender, b"usr\nboot");
                        } else if path == b"/usr" {
                            let _ = syscall::reply_message(sender, b"bin");
                        } else if path == b"/boot" {
                            let _ = syscall::reply_message(sender, b"modules");
                        } else if path == b"/usr/bin" {
                            let _ = syscall::reply_message(sender, list);
                        } else if path == b"/boot/modules" {
                            let _ = syscall::reply_message(sender, list);
                        } else {
                            let _ = syscall::reply_message(sender, b"NOTFOUND");
                        }
                    } else {
                        let _ = syscall::reply_message(sender, b"ERR");
                    }
                    continue;
                }

                if let Some(path) = strip_prefix(data, b"read:") {
                    if let Some(name) = path_to_name(path) {
                        let mut out = [0u8; 196];
                        if let Some(n) = syscall::bootfs_read(name, &mut out) {
                            let _ = syscall::reply_message(sender, &out[..n]);
                        } else {
                            let _ = syscall::reply_message(sender, b"NOTFOUND");
                        }
                    } else {
                        let _ = syscall::reply_message(sender, b"NOTFOUND");
                    }
                    continue;
                }

                if let Some(path) = strip_prefix(data, b"resolve:") {
                    if let Some(name) = path_to_name(path) {
                        let _ = syscall::reply_message(sender, name);
                    } else {
                        let _ = syscall::reply_message(sender, b"NOTFOUND");
                    }
                    continue;
                }

                let _ = syscall::reply_message(sender, b"BADREQ");
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
}

fn path_to_name(path: &[u8]) -> Option<&[u8]> {
    if path == b"/" || path == b"/usr" || path == b"/boot" || path == b"/usr/bin" || path == b"/boot/modules" {
        return None;
    }
    if let Some(rest) = strip_prefix(path, b"/usr/bin/") {
        return Some(rest);
    }
    if let Some(rest) = strip_prefix(path, b"/boot/modules/") {
        return Some(rest);
    }
    None
}

fn strip_prefix<'a>(data: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    if data.len() < p.len() || &data[..p.len()] != p { None } else { Some(&data[p.len()..]) }
}
