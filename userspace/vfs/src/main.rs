#![no_std]
#![no_main]

use libminux::{syscall, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const MAX_MOUNTS: usize = 8;
const MAX_PATH: usize = 48;

#[derive(Clone, Copy)]
struct Mount {
    used: bool,
    path_len: usize,
    path: [u8; MAX_PATH],
    task_id: TaskId,
}

impl Mount {
    const fn empty() -> Self {
        Self { used: false, path_len: 0, path: [0; MAX_PATH], task_id: 0 }
    }
}

struct Vfs {
    mounts: [Mount; MAX_MOUNTS],
}

impl Vfs {
    const fn new() -> Self {
        Self { mounts: [Mount::empty(); MAX_MOUNTS] }
    }

    fn mount(&mut self, at: &[u8], task_id: TaskId) -> bool {
        if at.is_empty() || at.len() > MAX_PATH || at[0] != b'/' || task_id == 0 {
            return false;
        }
        for m in self.mounts.iter_mut() {
            if m.used && m.path_len == at.len() && m.path[..m.path_len] == *at {
                m.task_id = task_id;
                return true;
            }
        }
        for m in self.mounts.iter_mut() {
            if !m.used {
                m.used = true;
                m.path_len = at.len();
                m.path[..at.len()].copy_from_slice(at);
                m.task_id = task_id;
                return true;
            }
        }
        false
    }

    fn resolve_mount<'a>(&self, path: &'a [u8]) -> Option<(TaskId, &'a [u8])> {
        let mut best: Option<(usize, TaskId)> = None;
        for m in &self.mounts {
            if !m.used {
                continue;
            }
            let mp = &m.path[..m.path_len];
            if !path.starts_with(mp) {
                continue;
            }
            if path.len() > mp.len() && mp != b"/" && path[mp.len()] != b'/' {
                continue;
            }
            if best.is_none() || m.path_len > best.unwrap().0 {
                best = Some((m.path_len, m.task_id));
            }
        }
        let (prefix_len, task_id) = best?;
        let rem = if prefix_len == 1 { path } else { &path[prefix_len..] };
        let rewritten = if rem.is_empty() { b"/" as &[u8] } else { rem };
        Some((task_id, rewritten))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"register:vfs:vfs");
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"register:fs:vfs");

    let mut vfs = Vfs::new();
    wait_mounts(&mut vfs);

    loop {
        let mut buf = [0u8; 196];
        match syscall::receive_message(&mut buf) {
            Ok((sender, MessageType::Request)) => {
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let msg = &buf[..len];
                handle_req(&mut vfs, sender, msg);
            }
            Ok(_) => {}
            Err(_) => syscall::yield_cpu(),
        }
    }
}

fn wait_mounts(vfs: &mut Vfs) {
    for _ in 0..5_000_000u32 {
        if let Some(ramfs) = discover_service(b"lookup:ramfs") {
            let _ = vfs.mount(b"/", ramfs);
            return;
        }
        syscall::yield_cpu();
    }
}

fn handle_req(vfs: &mut Vfs, sender: TaskId, msg: &[u8]) {
    if let Some(rest) = strip_prefix(msg, b"mount:") {
        if let Some((at, class)) = split_once(rest, b':') {
            if let Some(task_id) = discover_class(class) {
                let ok = vfs.mount(at, task_id);
                let _ = syscall::reply_message(sender, if ok { b"OK" } else { b"ERR" });
                return;
            }
        }
        let _ = syscall::reply_message(sender, b"ERR");
        return;
    }

    // routed ops: resolve/read/list/create/mkdir/write
    if let Some((verb, path, payload)) = parse_path_op(msg) {
        if let Some((fs_task, rewritten)) = vfs.resolve_mount(path) {
            let mut req = [0u8; 196];
            let mut n = 0usize;
            req[..verb.len()].copy_from_slice(verb);
            n += verb.len();
            req[n..n + rewritten.len()].copy_from_slice(rewritten);
            n += rewritten.len();
            if let Some(data) = payload {
                if n < req.len() {
                    req[n] = b':';
                    n += 1;
                }
                let cp = core::cmp::min(data.len(), req.len().saturating_sub(n));
                req[n..n + cp].copy_from_slice(&data[..cp]);
                n += cp;
            }
            let _ = syscall::send_message(fs_task, MessageType::Request, &req[..n]);
            let mut resp = [0u8; 196];
            if let Ok((_from, MessageType::Reply)) = syscall::receive_message(&mut resp) {
                let rlen = resp.iter().position(|&b| b == 0).unwrap_or(resp.len());
                let _ = syscall::reply_message(sender, &resp[..rlen]);
            } else {
                let _ = syscall::reply_message(sender, b"ERR");
            }
            return;
        }
        let _ = syscall::reply_message(sender, b"NOTFOUND");
        return;
    }

    let _ = syscall::reply_message(sender, b"BADREQ");
}

fn parse_path_op<'a>(msg: &'a [u8]) -> Option<(&'static [u8], &'a [u8], Option<&'a [u8]>)> {
    if let Some(path) = strip_prefix(msg, b"resolve:") {
        return Some((b"resolve:", path, None));
    }
    if let Some(path) = strip_prefix(msg, b"read:") {
        return Some((b"read:", path, None));
    }
    if let Some(path) = strip_prefix(msg, b"list:") {
        return Some((b"list:", path, None));
    }
    if let Some(path) = strip_prefix(msg, b"create:") {
        return Some((b"create:", path, None));
    }
    if let Some(path) = strip_prefix(msg, b"mkdir:") {
        return Some((b"mkdir:", path, None));
    }
    if let Some(rest) = strip_prefix(msg, b"write:") {
        if let Some((path, payload)) = split_once(rest, b':') {
            return Some((b"write:", path, Some(payload)));
        }
    }
    None
}

fn discover_service(req: &[u8]) -> Option<TaskId> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, req);
    let mut reply = [0u8; 16];
    if let Ok((_sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
        let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
        return parse_u32_ascii(&reply[..len]);
    }
    None
}

fn discover_class(class: &[u8]) -> Option<TaskId> {
    let mut req = [0u8; 32];
    req[..7].copy_from_slice(b"lookup:");
    let n = core::cmp::min(class.len(), req.len() - 7);
    req[7..7 + n].copy_from_slice(&class[..n]);
    discover_service(&req[..7 + n])
}

fn parse_u32_ascii(data: &[u8]) -> Option<u32> {
    if data.is_empty() {
        return None;
    }
    let mut v = 0u32;
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

fn split_once<'a>(data: &'a [u8], delim: u8) -> Option<(&'a [u8], &'a [u8])> {
    let pos = data.iter().position(|&b| b == delim)?;
    Some((&data[..pos], &data[pos + 1..]))
}
