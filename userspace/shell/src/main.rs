//! Interactive `sh` for Minux microkernel

#![no_std]
#![no_main]

use libminux::{syscall, MessageType};

const INIT_TASK_ID: u32 = 2;
const FALLBACK_LOADER_ID: u32 = 1;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let console_id = wait_for_console_service();
    let loader_id = discover_loader_service().unwrap_or(FALLBACK_LOADER_ID);

    loop {
        if !try_acquire_tty(console_id) {
            syscall::yield_cpu();
            continue;
        }

        let _ = syscall::send_message(console_id, MessageType::Notification, b"\nsh (minux backport) v0.1");
        let _ = syscall::send_message(console_id, MessageType::Notification, b"\ncommands: help clear status snake echo");
        let _ = syscall::send_message(console_id, MessageType::Notification, b"\nsh> ");

        let mut line = [0u8; 80];
        let mut line_len = 0usize;

        loop {
            let mut buf = [0u8; 32];
            match syscall::receive_message(&mut buf) {
                Ok((_sender, _msg_type)) => {
                    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                    let data = &buf[..len];
                    if data.len() == 5 && &data[..4] == b"key:" {
                        let ch = data[4];
                        match ch {
                            b'\n' => {
                                let _ = syscall::send_message(console_id, MessageType::Notification, b"\n");
                                execute_command(&line[..line_len], console_id, loader_id);
                                line_len = 0;
                                let _ = syscall::send_message(console_id, MessageType::Notification, b"\nsh> ");
                            }
                            0x08 => {
                                if line_len > 0 {
                                    line_len -= 1;
                                    let _ = syscall::send_message(console_id, MessageType::Notification, b"\x08 \x08");
                                }
                            }
                            b'\t' => {}
                            c => {
                                if is_printable(c) && line_len < line.len() {
                                    line[line_len] = c;
                                    line_len += 1;
                                    let echo = [c];
                                    let _ = syscall::send_message(console_id, MessageType::Notification, &echo);
                                }
                            }
                        }
                    }
                }
                Err(_) => syscall::yield_cpu(),
            }
        }
    }
}

fn execute_command(cmd: &[u8], console_id: u32, loader_id: u32) {
    let cmd = trim_ascii(cmd);
    if cmd.is_empty() {
        return;
    }

    let (name, args) = split_first_word(cmd);

    if name == b"help" {
        let _ = syscall::send_message(console_id, MessageType::Notification, b"Minux Shell Commands:");
        let _ = syscall::send_message(console_id, MessageType::Notification, b"\n  help   - Show this help");
        let _ = syscall::send_message(console_id, MessageType::Notification, b"\n  clear  - Clear screen");
        let _ = syscall::send_message(console_id, MessageType::Notification, b"\n  status - Show system status");
        let _ = syscall::send_message(console_id, MessageType::Notification, b"\n  snake  - Launch snake demo");
        let _ = syscall::send_message(console_id, MessageType::Notification, b"\n  echo   - Echo arguments");
        return;
    }

    if name == b"clear" {
        let _ = syscall::send_message(console_id, MessageType::Notification, b"clear");
        return;
    }

    if name == b"status" {
        let _ = syscall::send_message(console_id, MessageType::Notification, b"System Status:");
        let _ = syscall::send_message(console_id, MessageType::Notification, b"\n  Kernel: Minux Microkernel");
        let _ = syscall::send_message(console_id, MessageType::Notification, b"\n  Architecture: x86_64");
        let _ = syscall::send_message(console_id, MessageType::Notification, b"\n  Services: VESA Driver, Input, GFX, Console, Init, sh, Snake");
        return;
    }

    if name == b"echo" {
        if args.is_empty() {
            let _ = syscall::send_message(console_id, MessageType::Notification, b"(empty)");
        } else {
            let _ = syscall::send_message(console_id, MessageType::Notification, args);
        }
        return;
    }

    if name == b"snake" || name == b"./usr/bin/snake" {
        let _ = syscall::send_message(console_id, MessageType::Notification, b"Launching snake via elf_loader...");
        let _ = request_reply(loader_id, b"exec:/usr/bin/snake");
        return;
    }

    let _ = syscall::send_message(console_id, MessageType::Notification, b"Unknown command. Type 'help'.");
}

fn discover_loader_service() -> Option<u32> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"lookup:loader");
    let mut reply = [0u8; 16];
    if let Ok((sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
        if sender != INIT_TASK_ID {
            return None;
        }
        let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
        return parse_u32_ascii(&reply[..len]);
    }
    None
}

fn wait_for_console_service() -> u32 {
    loop {
        if let Some(id) = discover_console_service() {
            return id;
        }
        syscall::yield_cpu();
    }
}

fn discover_console_service() -> Option<u32> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"lookup:tty");
    let mut reply = [0u8; 16];
    if let Ok((sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
        if sender != INIT_TASK_ID {
            return None;
        }
        let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
        let id = parse_u32_ascii(&reply[..len])?;
        if id == INIT_TASK_ID || id == 1 {
            return None;
        }
        return Some(id);
    }
    None
}

fn try_acquire_tty(console_id: u32) -> bool {
    if let Some(reply) = request_reply(console_id, b"tty:acquire") {
        return starts_with(&reply, b"TTY:FG");
    }
    false
}

fn request_reply(target: u32, req: &[u8]) -> Option<[u8; 16]> {
    let _ = syscall::send_message(target, MessageType::Request, req);
    let mut reply = [0u8; 16];
    if let Ok((_sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
        return Some(reply);
    }
    None
}

fn starts_with(buf: &[u8], pat: &[u8]) -> bool {
    buf.len() >= pat.len() && &buf[..pat.len()] == pat
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

fn is_printable(c: u8) -> bool {
    (0x20..=0x7e).contains(&c)
}

fn is_space(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\r' || c == b'\n'
}

fn trim_ascii(mut s: &[u8]) -> &[u8] {
    while let Some(&b) = s.first() {
        if !is_space(b) {
            break;
        }
        s = &s[1..];
    }
    while let Some(&b) = s.last() {
        if !is_space(b) {
            break;
        }
        s = &s[..s.len() - 1];
    }
    s
}

fn split_first_word(s: &[u8]) -> (&[u8], &[u8]) {
    let mut i = 0usize;
    while i < s.len() && !is_space(s[i]) {
        i += 1;
    }
    let name = &s[..i];
    let mut rest = &s[i..];
    while let Some(&b) = rest.first() {
        if !is_space(b) {
            break;
        }
        rest = &rest[1..];
    }
    (name, rest)
}
