//! Init process for Minux microkernel.
//! Root-task model: bootstrap only (`elf_loader`, `init`) then userland-driven bring-up.

#![no_std]
#![no_main]

use libminux::{syscall, MessageType};
const MAX_REGISTRY: usize = 16;

#[derive(Clone, Copy)]
struct RegistryEntry {
    class: [u8; 16],
    class_len: usize,
    name: [u8; 24],
    name_len: usize,
    task_id: u32,
}

impl RegistryEntry {
    const fn empty() -> Self {
        Self {
            class: [0; 16],
            class_len: 0,
            name: [0; 24],
            name_len: 0,
            task_id: 0,
        }
    }
}

struct Registry {
    entries: [RegistryEntry; MAX_REGISTRY],
    count: usize,
}

impl Registry {
    const fn new() -> Self {
        Self {
            entries: [RegistryEntry::empty(); MAX_REGISTRY],
            count: 0,
        }
    }

    fn register(&mut self, class: &[u8], name: &[u8], task_id: u32) {
        if class.is_empty() || name.is_empty() {
            return;
        }
        for i in 0..self.count {
            let e = &mut self.entries[i];
            if e.class_len == class.len()
                && e.class[..e.class_len] == class[..]
                && e.name_len == name.len()
                && e.name[..e.name_len] == name[..]
            {
                e.task_id = task_id;
                return;
            }
        }
        if self.count >= MAX_REGISTRY {
            return;
        }
        let idx = self.count;
        self.count += 1;
        let e = &mut self.entries[idx];
        e.class_len = core::cmp::min(class.len(), e.class.len());
        e.class[..e.class_len].copy_from_slice(&class[..e.class_len]);
        e.name_len = core::cmp::min(name.len(), e.name.len());
        e.name[..e.name_len].copy_from_slice(&name[..e.name_len]);
        e.task_id = task_id;
    }

    fn lookup_class(&self, class: &[u8]) -> Option<u32> {
        for i in 0..self.count {
            let e = &self.entries[i];
            if e.class_len == class.len() && e.class[..e.class_len] == class[..] {
                return Some(e.task_id);
            }
        }
        None
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    init_system();
    supervision_loop();
}

fn init_system() {
}

fn supervision_loop() -> ! {
    let mut registry = Registry::new();
    launch_policy(&mut registry);
    loop {
        handle_message(&mut registry);
        syscall::yield_cpu();
    }
}

fn launch_policy(registry: &mut Registry) {
    const PLAN: [(&str, Option<&[u8]>); 10] = [
        ("ramfs", Some(b"ramfs")),
        ("bootfs_service", Some(b"bootfs")),
        ("vfs", Some(b"fs")),
        ("vga_driver", Some(b"display")),
        ("vesa_driver", Some(b"display_vesa")),
        ("gfx_service", Some(b"gfx")),
        ("input_service", Some(b"input")),
        ("console_service", Some(b"tty")),
        ("proc_service", Some(b"proc")),
        ("shell", None),
    ];

    for (svc, class) in PLAN {
        if !start_service(registry, svc) {
            continue;
        }
        if let Some(class_name) = class {
            wait_for_class(registry, class_name);
        }
    }
}

fn start_service(registry: &mut Registry, service_name: &str) -> bool {
    let name = service_name.as_bytes();
    if name.is_empty() {
        return false;
    }
    match syscall::exec_module(name) {
        Ok(_task_id) => true,
        Err(_) => {
            // Keep servicing registration traffic even on failed launch.
            for _ in 0..100_000u32 {
                handle_message(registry);
                syscall::yield_cpu();
            }
            false
        }
    }
}

fn wait_for_class(registry: &mut Registry, class: &[u8]) {
    for _ in 0..5_000_000u32 {
        if registry.lookup_class(class).is_some() {
            return;
        }
        handle_message(registry);
        syscall::yield_cpu();
    }
}

fn handle_message(registry: &mut Registry) {
    if let Some((sender, msg_type, len, data)) = receive_message_raw() {
        handle_message_raw(registry, sender, msg_type, &data[..len]);
    }
}

fn receive_message_raw() -> Option<(u32, MessageType, usize, [u8; 160])> {
    let mut buffer = [0u8; 160];
    if let Ok((sender, msg_type)) = syscall::receive_message(&mut buffer) {
        let len = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
        Some((sender, msg_type, len, buffer))
    } else {
        None
    }
}

fn handle_message_raw(registry: &mut Registry, sender: u32, msg_type: MessageType, data: &[u8]) {
    if !matches!(msg_type, MessageType::Request) {
        return;
    }

    // register:<class>:<name>
    if let Some(rest) = strip_prefix(data, b"register:") {
        if let Some((class, name)) = split_once(rest, b':') {
            registry.register(class, name, sender);
            let _ = syscall::reply_message(sender, b"REG:OK");
            return;
        }
    }

    // lookup:<class>
    if let Some(class) = strip_prefix(data, b"lookup:") {
        if let Some(task_id) = registry.lookup_class(class) {
            let mut buf = [0u8; 16];
            let n = write_u32_ascii(task_id, &mut buf);
            let _ = syscall::reply_message(sender, &buf[..n]);
        } else {
            let _ = syscall::reply_message(sender, b"0");
        }
    }
}

fn strip_prefix<'a>(data: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    if data.len() < p.len() || &data[..p.len()] != p {
        None
    } else {
        Some(&data[p.len()..])
    }
}

fn split_once<'a>(data: &'a [u8], delim: u8) -> Option<(&'a [u8], &'a [u8])> {
    let pos = data.iter().position(|&b| b == delim)?;
    Some((&data[..pos], &data[pos + 1..]))
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
