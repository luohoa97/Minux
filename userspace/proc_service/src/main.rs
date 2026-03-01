#![no_std]
#![no_main]

use libminux::{syscall, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const TASK_TERMINATED: u32 = 5;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"register:proc:proc_service");

    loop {
        let mut buf = [0u8; 128];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                if !matches!(msg_type, MessageType::Request) {
                    continue;
                }
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let data = &buf[..len];

                if let Some(name) = strip_prefix(data, b"proc:exec:") {
                    match syscall::exec_module(name) {
                        Ok(tid) => {
                            let mut out = [0u8; 16];
                            let n = write_u32_ascii(tid, &mut out);
                            let _ = syscall::reply_message(sender, &out[..n]);
                        }
                        Err(_) => {
                            let _ = syscall::reply_message(sender, b"ERR");
                        }
                    }
                    continue;
                }

                if let Some(tid_bytes) = strip_prefix(data, b"proc:wait:") {
                    if let Some(tid) = parse_u32_ascii(tid_bytes) {
                        loop {
                            if let Some((state, code)) = syscall::get_task_info(tid) {
                                if state == TASK_TERMINATED {
                                    let mut out = [0u8; 32];
                                    let mut n = 0usize;
                                    n += write_ascii(&mut out[n..], b"exit:");
                                    n += write_u64_ascii(code, &mut out[n..]);
                                    let _ = syscall::reply_message(sender, &out[..n]);
                                    break;
                                }
                            } else {
                                let _ = syscall::reply_message(sender, b"ERR");
                                break;
                            }
                            syscall::yield_cpu();
                        }
                    } else {
                        let _ = syscall::reply_message(sender, b"BADREQ");
                    }
                    continue;
                }

                if data == b"proc:fork" {
                    let _ = syscall::reply_message(sender, b"ERR:FORK");
                    continue;
                }

                let _ = syscall::reply_message(sender, b"BADREQ");
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
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

fn write_u64_ascii(id: u64, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    if id == 0 {
        out[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 20];
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
