#![no_std]
#![no_main]

use libminux::{syscall, MessageType};

const INIT_TASK_ID: u32 = 2;
const MAX_NODES: usize = 128;
const MAX_NAME: usize = 32;
const MAX_CHILDREN: usize = 16;
const MAX_DATA: usize = 192;

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Unused = 0,
    File = 1,
    Dir = 2,
}

#[derive(Clone, Copy)]
struct Node {
    kind: NodeKind,
    parent: u16,
    name_len: u8,
    name: [u8; MAX_NAME],
    child_count: u8,
    children: [u16; MAX_CHILDREN],
    data_len: u16,
    data: [u8; MAX_DATA],
}

impl Node {
    const fn empty() -> Self {
        Self {
            kind: NodeKind::Unused,
            parent: 0,
            name_len: 0,
            name: [0; MAX_NAME],
            child_count: 0,
            children: [0; MAX_CHILDREN],
            data_len: 0,
            data: [0; MAX_DATA],
        }
    }
}

struct RamFs {
    nodes: [Node; MAX_NODES],
}

impl RamFs {
    const fn new() -> Self {
        Self {
            nodes: [Node::empty(); MAX_NODES],
        }
    }

    fn init(&mut self) {
        self.nodes[0].kind = NodeKind::Dir;
        self.nodes[0].name_len = 1;
        self.nodes[0].name[0] = b'/';
        let _ = self.mkdir_p(b"/usr/bin");
        let _ = self.mkdir_p(b"/usr/share/kbd/consolefonts");
        let _ = self.mkdir_p(b"/boot/modules");
        let _ = self.write_file(b"/usr/bin/snake", b"snake");
        let _ = self.write_file(b"/usr/bin/shell", b"shell");
        let _ = self.write_file(b"/usr/bin/sh", b"shell");
        let _ = self.write_file(b"/usr/bin/x11_demo", b"x11_demo");
        let _ = self.write_file(b"/usr/bin/ls", b"ls");
        let _ = self.write_file(b"/usr/bin/cat", b"cat");
        let _ = self.write_file(b"/usr/bin/tree", b"tree");
        let _ = self.write_file(b"/usr/share/kbd/consolefonts/ter-u16n.bdf", b"usr/share/kbd/consolefonts/ter-u16n.bdf");
        let _ = self.write_file(b"/boot/modules/init", b"init");
        let _ = self.write_file(b"/boot/modules/elf_loader", b"elf_loader");
    }

    fn find_child(&self, parent: usize, name: &[u8]) -> Option<usize> {
        let p = &self.nodes[parent];
        let n = p.child_count as usize;
        let mut i = 0usize;
        while i < n {
            let idx = p.children[i] as usize;
            let c = &self.nodes[idx];
            if c.kind != NodeKind::Unused
                && c.name_len as usize == name.len()
                && c.name[..name.len()] == *name
            {
                return Some(idx);
            }
            i += 1;
        }
        None
    }

    fn alloc_node(&mut self, parent: usize, name: &[u8], kind: NodeKind) -> Option<usize> {
        if name.is_empty() || name.len() > MAX_NAME {
            return None;
        }
        if self.nodes[parent].kind != NodeKind::Dir || self.nodes[parent].child_count as usize >= MAX_CHILDREN {
            return None;
        }
        let mut idx = 1usize;
        while idx < MAX_NODES {
            if self.nodes[idx].kind == NodeKind::Unused {
                let n = &mut self.nodes[idx];
                n.kind = kind;
                n.parent = parent as u16;
                n.name_len = name.len() as u8;
                n.name[..name.len()].copy_from_slice(name);
                let c = self.nodes[parent].child_count as usize;
                self.nodes[parent].children[c] = idx as u16;
                self.nodes[parent].child_count += 1;
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    fn split_next<'a>(path: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
        while *at < path.len() && path[*at] == b'/' {
            *at += 1;
        }
        if *at >= path.len() {
            return None;
        }
        let start = *at;
        while *at < path.len() && path[*at] != b'/' {
            *at += 1;
        }
        Some(&path[start..*at])
    }

    fn walk(&self, path: &[u8]) -> Option<usize> {
        if path == b"/" {
            return Some(0);
        }
        if path.is_empty() || path[0] != b'/' {
            return None;
        }
        let mut cur = 0usize;
        let mut at = 0usize;
        while let Some(seg) = Self::split_next(path, &mut at) {
            cur = self.find_child(cur, seg)?;
        }
        Some(cur)
    }

    fn walk_parent<'a>(&self, path: &'a [u8]) -> Option<(usize, &'a [u8])> {
        if path.is_empty() || path[0] != b'/' {
            return None;
        }
        let mut at = 0usize;
        let mut cur = 0usize;
        let mut last = None;
        while let Some(seg) = Self::split_next(path, &mut at) {
            last = Some(seg);
            if at < path.len() {
                cur = self.find_child(cur, seg)?;
            }
        }
        Some((cur, last?))
    }

    fn mkdir_p(&mut self, path: &[u8]) -> bool {
        if path.is_empty() || path[0] != b'/' {
            return false;
        }
        let mut cur = 0usize;
        let mut at = 0usize;
        while let Some(seg) = Self::split_next(path, &mut at) {
            if let Some(n) = self.find_child(cur, seg) {
                if self.nodes[n].kind != NodeKind::Dir {
                    return false;
                }
                cur = n;
            } else if let Some(new_idx) = self.alloc_node(cur, seg, NodeKind::Dir) {
                cur = new_idx;
            } else {
                return false;
            }
        }
        true
    }

    fn create_file(&mut self, path: &[u8]) -> bool {
        let (parent, name) = match self.walk_parent(path) {
            Some(x) => x,
            None => return false,
        };
        if self.find_child(parent, name).is_some() {
            return false;
        }
        self.alloc_node(parent, name, NodeKind::File).is_some()
    }

    fn write_file(&mut self, path: &[u8], data: &[u8]) -> bool {
        if data.len() > MAX_DATA {
            return false;
        }
        let idx = if let Some(i) = self.walk(path) {
            i
        } else {
            let (parent, name) = match self.walk_parent(path) {
                Some(x) => x,
                None => return false,
            };
            match self.alloc_node(parent, name, NodeKind::File) {
                Some(i) => i,
                None => return false,
            }
        };
        let n = &mut self.nodes[idx];
        if n.kind != NodeKind::File {
            return false;
        }
        n.data_len = data.len() as u16;
        n.data[..data.len()].copy_from_slice(data);
        true
    }

    fn read_file<'a>(&'a self, path: &[u8]) -> Option<&'a [u8]> {
        let idx = self.walk(path)?;
        let n = &self.nodes[idx];
        if n.kind != NodeKind::File {
            return None;
        }
        Some(&n.data[..n.data_len as usize])
    }

    fn list_into(&self, path: &[u8], out: &mut [u8]) -> usize {
        let idx = match self.walk(path) {
            Some(i) => i,
            None => return 0,
        };
        let dir = &self.nodes[idx];
        if dir.kind != NodeKind::Dir {
            return 0;
        }
        let mut nout = 0usize;
        let mut i = 0usize;
        while i < dir.child_count as usize {
            let c = &self.nodes[dir.children[i] as usize];
            if i > 0 && nout < out.len() {
                out[nout] = b'\n';
                nout += 1;
            }
            let clen = c.name_len as usize;
            let copy = core::cmp::min(clen, out.len().saturating_sub(nout));
            out[nout..nout + copy].copy_from_slice(&c.name[..copy]);
            nout += copy;
            i += 1;
        }
        nout
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"register:ramfs:ramfs");
    let mut fs = RamFs::new();
    fs.init();

    loop {
        let mut buf = [0u8; 128];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                if !matches!(msg_type, MessageType::Request) {
                    continue;
                }
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let data = &buf[..len];

                if let Some(path) = strip_prefix(data, b"resolve:") {
                    let reply = fs.read_file(path).unwrap_or(b"NOTFOUND");
                    let _ = syscall::reply_message(sender, reply);
                } else if let Some(path) = strip_prefix(data, b"create:") {
                    let ok = fs.create_file(path);
                    let _ = syscall::reply_message(sender, if ok { b"OK" } else { b"ERR" });
                } else if let Some(path) = strip_prefix(data, b"mkdir:") {
                    let ok = fs.mkdir_p(path);
                    let _ = syscall::reply_message(sender, if ok { b"OK" } else { b"ERR" });
                } else if let Some(path) = strip_prefix(data, b"read:") {
                    let reply = fs.read_file(path).unwrap_or(b"NOTFOUND");
                    let _ = syscall::reply_message(sender, reply);
                } else if let Some(rest) = strip_prefix(data, b"write:") {
                    if let Some((path, payload)) = split_once(rest, b':') {
                        let ok = fs.write_file(path, payload);
                        let _ = syscall::reply_message(sender, if ok { b"OK" } else { b"ERR" });
                    } else {
                        let _ = syscall::reply_message(sender, b"BADREQ");
                    }
                } else if let Some(path) = strip_prefix(data, b"list:") {
                    let mut out = [0u8; 192];
                    let n = fs.list_into(path, &mut out);
                    if n == 0 {
                        let _ = syscall::reply_message(sender, b"NOTFOUND");
                    } else {
                        let _ = syscall::reply_message(sender, &out[..n]);
                    }
                } else {
                    let _ = syscall::reply_message(sender, b"BADREQ");
                }
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
}

fn strip_prefix<'a>(data: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    if data.len() < p.len() || &data[..p.len()] != p { None } else { Some(&data[p.len()..]) }
}

fn split_once<'a>(data: &'a [u8], delim: u8) -> Option<(&'a [u8], &'a [u8])> {
    let pos = data.iter().position(|&b| b == delim)?;
    Some((&data[..pos], &data[pos + 1..]))
}
