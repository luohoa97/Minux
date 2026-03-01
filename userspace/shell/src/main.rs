//! Interactive `sh` for Minux microkernel

#![no_std]
#![no_main]

use libminux::{syscall, MessageType};

const INIT_TASK_ID: u32 = 2;
const FALLBACK_LOADER_ID: u32 = 1;
const FALLBACK_FS_ID: u32 = 4;
const FALLBACK_PROC_ID: u32 = 0;
const PROMPT: &[u8] = b"sh> ";
const MAX_LINE: usize = 256;
const HISTORY_CAP: usize = 16;

struct Editor {
    line: [u8; MAX_LINE],
    len: usize,
    cursor: usize,
    rendered_len: usize,
    rendered_cursor: usize,
    history: [[u8; MAX_LINE]; HISTORY_CAP],
    history_len: [usize; HISTORY_CAP],
    history_count: usize,
    history_nav: Option<usize>,
    draft: [u8; MAX_LINE],
    draft_len: usize,
    esc_state: u8,
}

impl Editor {
    const fn new() -> Self {
        Self {
            line: [0; MAX_LINE],
            len: 0,
            cursor: 0,
            rendered_len: 0,
            rendered_cursor: 0,
            history: [[0; MAX_LINE]; HISTORY_CAP],
            history_len: [0; HISTORY_CAP],
            history_count: 0,
            history_nav: None,
            draft: [0; MAX_LINE],
            draft_len: 0,
            esc_state: 0,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let console_id = wait_for_console_service();
    let loader_id = discover_loader_service().unwrap_or(FALLBACK_LOADER_ID);
    let fs_id = discover_fs_service().unwrap_or(FALLBACK_FS_ID);
    let proc_id = discover_proc_service().unwrap_or(FALLBACK_PROC_ID);
    let mut cwd = [0u8; 96];
    let mut cwd_len = 1usize;
    cwd[0] = b'/';

    loop {
        if !try_acquire_tty(console_id) {
            syscall::yield_cpu();
            continue;
        }

        send_console(console_id, b"\nsh (minux backport) v0.1");
        send_console(console_id, b"\ncommands: help clear status snake echo set");
        send_console(console_id, b"\nsh> _\x08");

        let mut ed = Editor::new();
        ed.rendered_len = PROMPT.len();
        ed.rendered_cursor = PROMPT.len();

        loop {
            let mut buf = [0u8; 32];
            match syscall::receive_message(&mut buf) {
                Ok((_sender, msg_type)) => {
                    if !matches!(msg_type, MessageType::Notification | MessageType::Interrupt) {
                        continue;
                    }
                    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                    let data = &buf[..len];
                    consume_key_notifications(
                        data,
                        &mut ed,
                        console_id,
                        loader_id,
                        fs_id,
                        proc_id,
                        &mut cwd,
                        &mut cwd_len,
                    );
                }
                Err(_) => syscall::yield_cpu(),
            }
        }
    }
}

fn consume_key_notifications(
    data: &[u8],
    ed: &mut Editor,
    console_id: u32,
    loader_id: u32,
    fs_id: u32,
    proc_id: u32,
    cwd: &mut [u8; 96],
    cwd_len: &mut usize,
) {
    // Supports concatenated legacy "key:<byte>" packets and normalized raw bytes.
    let mut i = 0usize;
    while i < data.len() {
        let b = if i + 4 < data.len() && &data[i..i + 4] == b"key:" {
            i += 5;
            data[i - 1]
        } else {
            let v = data[i];
            i += 1;
            v
        };
        handle_key(b, ed, console_id, loader_id, fs_id, proc_id, cwd, cwd_len);
    }
}

fn handle_key(
    ch: u8,
    ed: &mut Editor,
    console_id: u32,
    loader_id: u32,
    fs_id: u32,
    proc_id: u32,
    cwd: &mut [u8; 96],
    cwd_len: &mut usize,
) {
    if ed.esc_state == 1 {
        ed.esc_state = if ch == b'[' { 2 } else { 0 };
        return;
    }
    if ed.esc_state == 2 {
        match ch {
            b'A' => history_prev(ed),
            b'B' => history_next(ed),
            b'C' => {
                if ed.cursor < ed.len {
                    ed.cursor += 1;
                }
            }
            b'D' => {
                if ed.cursor > 0 {
                    ed.cursor -= 1;
                }
            }
            _ => {}
        }
        ed.esc_state = 0;
        redraw_line(ed, console_id);
        return;
    }

    match ch {
        0x1b => {
            ed.esc_state = 1;
        }
        b'\n' | b'\r' => {
            send_console(console_id, b"\n");
            let mut cmd_buf = [0u8; MAX_LINE];
            cmd_buf[..ed.len].copy_from_slice(&ed.line[..ed.len]);
            let cmd = &cmd_buf[..ed.len];
            let trimmed = trim_ascii(cmd);
            if !trimmed.is_empty() {
                push_history(ed, trimmed);
            }
            execute_command(cmd, console_id, loader_id, fs_id, proc_id, cwd, cwd_len);
            ed.len = 0;
            ed.cursor = 0;
            ed.history_nav = None;
            ed.draft_len = 0;
            ed.rendered_len = 0;
            ed.rendered_cursor = 0;
            send_console(console_id, b"\nsh> _\x08");
            ed.rendered_len = PROMPT.len();
            ed.rendered_cursor = PROMPT.len();
        }
        0x08 => {
            if ed.cursor > 0 {
                for i in ed.cursor..ed.len {
                    ed.line[i - 1] = ed.line[i];
                }
                ed.len -= 1;
                ed.cursor -= 1;
                ed.history_nav = None;
                redraw_line(ed, console_id);
            }
        }
        b'\t' => {}
        c if is_printable(c) => {
            if ed.len < ed.line.len() {
                for i in (ed.cursor..ed.len).rev() {
                    ed.line[i + 1] = ed.line[i];
                }
                ed.line[ed.cursor] = c;
                ed.len += 1;
                ed.cursor += 1;
                ed.history_nav = None;
                redraw_line(ed, console_id);
            }
        }
        _ => {}
    }
}

fn redraw_line(ed: &mut Editor, console_id: u32) {
    send_repeat(console_id, b'\x08', ed.rendered_cursor);
    send_repeat(console_id, b' ', ed.rendered_len);
    send_repeat(console_id, b'\x08', ed.rendered_len);
    send_console(console_id, PROMPT);
    send_console(console_id, &ed.line[..ed.len]);
    let tail = ed.len.saturating_sub(ed.cursor);
    send_repeat(console_id, b'\x08', tail);
    // Cursor marker at logical cursor position.
    send_console(console_id, b"_\x08");
    ed.rendered_len = PROMPT.len() + ed.len;
    ed.rendered_cursor = PROMPT.len() + ed.cursor;
}

fn send_repeat(console_id: u32, ch: u8, mut count: usize) {
    let mut chunk = [0u8; 64];
    chunk.fill(ch);
    while count > 0 {
        let n = core::cmp::min(count, chunk.len());
        send_console(console_id, &chunk[..n]);
        count -= n;
    }
}

fn push_history(ed: &mut Editor, cmd: &[u8]) {
    if ed.history_count > 0 {
        let last = ed.history_count - 1;
        if ed.history_len[last] == cmd.len() && ed.history[last][..cmd.len()] == cmd[..] {
            return;
        }
    }
    if ed.history_count < HISTORY_CAP {
        let idx = ed.history_count;
        ed.history[idx][..cmd.len()].copy_from_slice(cmd);
        ed.history_len[idx] = cmd.len();
        ed.history_count += 1;
    } else {
        for i in 1..HISTORY_CAP {
            ed.history[i - 1] = ed.history[i];
            ed.history_len[i - 1] = ed.history_len[i];
        }
        let idx = HISTORY_CAP - 1;
        ed.history[idx][..cmd.len()].copy_from_slice(cmd);
        ed.history_len[idx] = cmd.len();
    }
}

fn history_prev(ed: &mut Editor) {
    if ed.history_count == 0 {
        return;
    }
    if ed.history_nav.is_none() {
        ed.draft[..ed.len].copy_from_slice(&ed.line[..ed.len]);
        ed.draft_len = ed.len;
        ed.history_nav = Some(ed.history_count - 1);
    } else if let Some(pos) = ed.history_nav {
        if pos > 0 {
            ed.history_nav = Some(pos - 1);
        }
    }
    if let Some(pos) = ed.history_nav {
        let n = ed.history_len[pos];
        ed.line[..n].copy_from_slice(&ed.history[pos][..n]);
        ed.len = n;
        ed.cursor = n;
    }
}

fn history_next(ed: &mut Editor) {
    let Some(pos) = ed.history_nav else { return; };
    if pos + 1 < ed.history_count {
        let np = pos + 1;
        ed.history_nav = Some(np);
        let n = ed.history_len[np];
        ed.line[..n].copy_from_slice(&ed.history[np][..n]);
        ed.len = n;
        ed.cursor = n;
    } else {
        ed.history_nav = None;
        ed.line[..ed.draft_len].copy_from_slice(&ed.draft[..ed.draft_len]);
        ed.len = ed.draft_len;
        ed.cursor = ed.len;
    }
}

fn execute_command(
    cmd: &[u8],
    console_id: u32,
    loader_id: u32,
    fs_id: u32,
    proc_id: u32,
    cwd: &mut [u8; 96],
    cwd_len: &mut usize,
) {
    let cmd = trim_ascii(cmd);
    if cmd.is_empty() {
        return;
    }

    let (name, args) = split_first_word(cmd);
    let cwd_path_len = *cwd_len;

    if name == b"help" {
        send_console(console_id, b"Minux Shell Commands:");
        send_console(console_id, b"\n  help   - Show this help");
        send_console(console_id, b"\n  clear  - Clear screen");
        send_console(console_id, b"\n  status - Show system status");
        send_console(console_id, b"\n  snake  - Launch snake demo");
        send_console(console_id, b"\n  echo   - Echo arguments");
        send_console(console_id, b"\n  set    - Set/show env (PATH)");
        send_console(console_id, b"\n  cd     - Change directory");
        return;
    }

    if name == b"clear" {
        send_console(console_id, b"clear");
        return;
    }

    if name == b"status" {
        send_console(console_id, b"System Status:");
        send_console(console_id, b"\n  Kernel: Minux Microkernel");
        send_console(console_id, b"\n  Architecture: x86_64");
        send_console(console_id, b"\n  Services: VESA Driver, Input, GFX, Console, Init, sh, Snake");
        return;
    }

    if name == b"echo" {
        if args.is_empty() {
            send_console(console_id, b"(empty)");
        } else {
            send_console(console_id, args);
        }
        return;
    }

    let _ = fs_id;

    if name == b"set" {
        if args.is_empty() {
            send_console(console_id, b"\nPATH=/usr/bin:/bin");
            return;
        }
        if let Some((key, val)) = split_once(args, b'=') {
            if key == b"PATH" {
                // PATH is fixed for now; accept but ignore custom values.
                let _ = val;
                send_console(console_id, b"\nPATH=/usr/bin:/bin");
                return;
            }
        }
        send_console(console_id, b"\nusage: set PATH=/usr/bin:/bin");
        return;
    }

    if name == b"cd" {
        let target = if args.is_empty() { b"/" } else { args };
        let mut cwd_buf = [0u8; 96];
        let n = core::cmp::min(cwd_path_len, cwd_buf.len());
        cwd_buf[..n].copy_from_slice(&cwd[..n]);
        let cwd_path = &cwd_buf[..n];
        if let Some(n) = normalize_path(target, cwd_path, cwd) {
            *cwd_len = n;
        } else {
            send_console(console_id, b"\ncd: invalid path");
        }
        return;
    }

    if name == b"snake" {
        send_console(console_id, b"Launching snake via elf_loader...");
        if request_reply(loader_id, b"exec:/usr/bin/snake").is_none() {
            send_console(console_id, b"\n[sh] snake launch failed");
        }
        return;
    }

    // Generic exec for explicit paths (absolute or relative).
    if name.contains(&b'/') {
        let cwd_path = &cwd[..cwd_path_len];
        let mut tmp = [0u8; 96];
        let full = if name.starts_with(b"/") {
            name
        } else if let Some(n) = normalize_path(name, cwd_path, &mut tmp) {
            &tmp[..n]
        } else {
            send_console(console_id, b"\n[sh] bad path");
            return;
        };
        // Paths require vfs resolve via elf_loader (proc_service can't resolve paths yet).
        if exec_with_args(loader_id, 0, full, args).is_none() {
            send_console(console_id, b"\n[sh] exec failed");
        }
        return;
    }

    let mut args_buf = [0u8; 96];
    let mut args_len: usize = 0;
    let args = if args.is_empty() && (name == b"ls" || name == b"tree") {
        let n = core::cmp::min(cwd_path_len, args_buf.len());
        args_buf[..n].copy_from_slice(&cwd[..n]);
        args_len = n;
        &args_buf[..args_len]
    } else if !args.is_empty() && !args.starts_with(b"/") {
        let cwd_path = &cwd[..cwd_path_len];
        if let Some(n) = normalize_path(args, cwd_path, &mut args_buf) {
            args_len = n;
            &args_buf[..args_len]
        } else {
            send_console(console_id, b"\n[sh] bad path");
            return;
        }
    } else {
        args
    };

    // PATH search: /usr/bin and /bin.
    if let Some(tid) = exec_with_args(loader_id, proc_id, name, args) {
        if proc_id != 0 {
            wait_child(proc_id, tid);
        }
        return;
    }
    let mut path = [0u8; 96];
    let mut n = 0usize;
    n += write_ascii(&mut path[n..], b"/usr/bin/");
    n += write_ascii(&mut path[n..], name);
    if let Some(tid) = exec_with_args(loader_id, proc_id, &path[..n], args) {
        if proc_id != 0 {
            wait_child(proc_id, tid);
        }
        return;
    }
    n = 0;
    n += write_ascii(&mut path[n..], b"/bin/");
    n += write_ascii(&mut path[n..], name);
    if let Some(tid) = exec_with_args(loader_id, proc_id, &path[..n], args) {
        if proc_id != 0 {
            wait_child(proc_id, tid);
        }
        return;
    }
    send_console(console_id, b"Unknown command. Type 'help'.");
}

fn normalize_path(input: &[u8], cwd: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut stack: [[u8; 32]; 8] = [[0; 32]; 8];
    let mut lens: [usize; 8] = [0; 8];
    let mut depth = 0usize;

    let mut push = |seg: &[u8]| {
        if seg.is_empty() || seg == b"." {
            return;
        }
        if seg == b".." {
            if depth > 0 {
                depth -= 1;
            }
            return;
        }
        if depth < stack.len() {
            let n = core::cmp::min(seg.len(), stack[0].len());
            stack[depth][..n].copy_from_slice(&seg[..n]);
            lens[depth] = n;
            depth += 1;
        }
    };

    if !input.starts_with(b"/") {
        for seg in cwd.split(|&b| b == b'/') {
            push(seg);
        }
    }
    for seg in input.split(|&b| b == b'/') {
        push(seg);
    }

    let mut n = 0usize;
    if n < out.len() {
        out[n] = b'/';
        n += 1;
    }
    for i in 0..depth {
        if i > 0 {
            if n < out.len() {
                out[n] = b'/';
                n += 1;
            }
        }
        let len = core::cmp::min(lens[i], out.len().saturating_sub(n));
        out[n..n + len].copy_from_slice(&stack[i][..len]);
        n += len;
    }
    if n == 0 {
        return None;
    }
    Some(n)
}

fn wait_child(proc_id: u32, tid: u32) {
    let mut req = [0u8; 32];
    let mut n = 0usize;
    n += write_ascii(&mut req[n..], b"proc:wait:");
    n += write_u32_ascii(tid, &mut req[n..]);
    let _ = request_reply(proc_id, &req[..n]);
}

fn discover_loader_service() -> Option<u32> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"lookup:loader");
    for _ in 0..256u32 {
        let mut reply = [0u8; 16];
        if let Ok((sender, msg_type)) = syscall::receive_message(&mut reply) {
            if sender != INIT_TASK_ID || !matches!(msg_type, MessageType::Reply) {
                continue;
            }
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            return parse_u32_ascii(&reply[..len]);
        }
        syscall::yield_cpu();
    }
    None
}

fn discover_fs_service() -> Option<u32> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"lookup:fs");
    for _ in 0..256u32 {
        let mut reply = [0u8; 16];
        if let Ok((sender, msg_type)) = syscall::receive_message(&mut reply) {
            if sender != INIT_TASK_ID || !matches!(msg_type, MessageType::Reply) {
                continue;
            }
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            return parse_u32_ascii(&reply[..len]);
        }
        syscall::yield_cpu();
    }
    None
}

fn discover_proc_service() -> Option<u32> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"lookup:proc");
    for _ in 0..256u32 {
        let mut reply = [0u8; 16];
        if let Ok((sender, msg_type)) = syscall::receive_message(&mut reply) {
            if sender != INIT_TASK_ID || !matches!(msg_type, MessageType::Reply) {
                continue;
            }
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            return parse_u32_ascii(&reply[..len]);
        }
        syscall::yield_cpu();
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
    for _ in 0..256u32 {
        let mut reply = [0u8; 16];
        if let Ok((sender, msg_type)) = syscall::receive_message(&mut reply) {
            if sender != INIT_TASK_ID || !matches!(msg_type, MessageType::Reply) {
                continue;
            }
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            let id = parse_u32_ascii(&reply[..len])?;
            if id == INIT_TASK_ID || id == 1 {
                return None;
            }
            return Some(id);
        }
        syscall::yield_cpu();
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
    for _ in 0..512u32 {
        let mut reply = [0u8; 16];
        if let Ok((sender, msg_type)) = syscall::receive_message(&mut reply) {
            if sender == target && matches!(msg_type, MessageType::Reply) {
                return Some(reply);
            }
        }
        syscall::yield_cpu();
    }
    None
}

fn exec_with_args(loader_id: u32, proc_id: u32, name: &[u8], args: &[u8]) -> Option<u32> {
    let mut req = [0u8; 96];
    let mut n = 0usize;
    if proc_id != 0 {
        n += write_ascii(&mut req[n..], b"proc:exec:");
        n += write_ascii(&mut req[n..], name);
    } else {
        n += write_ascii(&mut req[n..], b"exec:");
        n += write_ascii(&mut req[n..], name);
    }
    let reply = if proc_id != 0 {
        request_reply(proc_id, &req[..n])?
    } else {
        request_reply(loader_id, &req[..n])?
    };
    let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
    let tid = parse_u32_ascii(&reply[..len])?;
    if !args.is_empty() {
        let mut argmsg = [0u8; 96];
        let mut k = 0usize;
        k += write_ascii(&mut argmsg[k..], b"args:");
        k += write_ascii(&mut argmsg[k..], args);
        let _ = syscall::send_message(tid, MessageType::Request, &argmsg[..k]);
    }
    Some(tid)
}

fn send_console(console_id: u32, data: &[u8]) {
    for _ in 0..64 {
        if syscall::send_message(console_id, MessageType::Notification, data).is_ok() {
            return;
        }
        syscall::yield_cpu();
    }
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

fn split_once<'a>(data: &'a [u8], delim: u8) -> Option<(&'a [u8], &'a [u8])> {
    let pos = data.iter().position(|&b| b == delim)?;
    Some((&data[..pos], &data[pos + 1..]))
}

fn write_ascii(out: &mut [u8], s: &[u8]) -> usize {
    let n = core::cmp::min(out.len(), s.len());
    out[..n].copy_from_slice(&s[..n]);
    n
}
