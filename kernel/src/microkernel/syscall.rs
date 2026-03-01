//! System call interface for minux microkernel
//!
//! The microkernel provides minimal system calls:
//! - IPC operations (send, receive, reply)
//! - Task management (yield, exit)
//! - Memory operations (map, unmap)

use crate::microkernel::TaskId;
use crate::microkernel::BOOTFS_MAGIC;
use crate::ipc::{Message, MessageType, IpcError};
use crate::microkernel::MAX_TASKS;

const TRACE_SYSCALLS: bool = false;

#[inline]
fn debug_msg_preview(data: &[u8], len: usize) {
    if !TRACE_SYSCALLS {
        return;
    }
    let n = core::cmp::min(len, 32);
    if n == 0 {
        return;
    }
    let mut out = [0u8; 32];
    for i in 0..n {
        let b = data[i];
        out[i] = if (0x20..=0x7e).contains(&b) { b } else { b'.' };
    }
    if let Ok(s) = core::str::from_utf8(&out[..n]) {
        crate::serial_debugln!("[DBG] ipc preview '{}'", s);
    }
}

#[inline]
fn is_valid_user_ptr(ptr: u64, len: usize) -> bool {
    if len == 0 {
        return true;
    }
    if ptr == 0 {
        return false;
    }
    // Current bring-up model runs tasks in lower canonical range.
    const CANONICAL_LOW_MAX: u64 = 0x0000_7fff_ffff_ffff;
    match ptr.checked_add(len as u64 - 1) {
        Some(end) => end <= CANONICAL_LOW_MAX,
        None => false,
    }
}

/// System call numbers
#[derive(Debug, Clone, Copy)]
#[repr(u64)]
pub enum Syscall {
    /// Send IPC message
    Send = 1,
    /// Receive IPC message  
    Receive = 2,
    /// Reply to IPC message
    Reply = 3,
    /// Yield CPU to scheduler
    Yield = 4,
    /// Exit current task
    Exit = 5,
    /// Create new task
    CreateTask = 6,
    /// Map memory page
    MapPage = 7,
    /// Unmap memory page
    UnmapPage = 8,
    /// Send IPC message (zero-copy descriptor)
    SendZc = 9,
    /// Receive IPC message (zero-copy descriptor)
    ReceiveZc = 10,
    /// Send short IPC message in registers (fast path)
    SendFast = 11,
    /// Receive short IPC message registers (fast path)
    ReceiveFast = 12,
    /// Execute deferred boot module by name (ELF load + task create)
    ExecModule = 13,
    /// Poll next keyboard scancode from IRQ queue
    ReadScancode = 14,
    /// Query boot framebuffer info (phys/pitch/width/height/bpp)
    GetFramebufferInfo = 15,
    /// Query task state/exit code
    GetTaskInfo = 16,
    /// Bootfs list/read
    BootfsList = 17,
    BootfsRead = 18,
}

impl Syscall {
    /// Convert from raw syscall number
    pub fn from_u64(n: u64) -> Option<Self> {
        match n {
            1 => Some(Self::Send),
            2 => Some(Self::Receive),
            3 => Some(Self::Reply),
            4 => Some(Self::Yield),
            5 => Some(Self::Exit),
            6 => Some(Self::CreateTask),
            7 => Some(Self::MapPage),
            8 => Some(Self::UnmapPage),
            9 => Some(Self::SendZc),
            10 => Some(Self::ReceiveZc),
            11 => Some(Self::SendFast),
            12 => Some(Self::ReceiveFast),
            13 => Some(Self::ExecModule),
            14 => Some(Self::ReadScancode),
            15 => Some(Self::GetFramebufferInfo),
            16 => Some(Self::GetTaskInfo),
            17 => Some(Self::BootfsList),
            18 => Some(Self::BootfsRead),
            _ => None,
        }
    }
}

/// System call result
pub type SyscallResult = Result<u64, SyscallError>;

/// System call errors
#[derive(Debug, Clone, Copy)]
pub enum SyscallError {
    InvalidSyscall,
    InvalidArgument,
    PermissionDenied,
    NoSuchTask,
    QueueFull,
    NoMessage,
}

impl From<IpcError> for SyscallError {
    fn from(err: IpcError) -> Self {
        match err {
            IpcError::InvalidTask => SyscallError::NoSuchTask,
            IpcError::QueueFull => SyscallError::QueueFull,
            IpcError::NoMessage => SyscallError::NoMessage,
            IpcError::PermissionDenied => SyscallError::PermissionDenied,
            IpcError::InvalidCapability => SyscallError::InvalidArgument,
        }
    }
}

/// Handle system call
pub fn handle_syscall(syscall_num: u64, args: &[u64; 6]) -> SyscallResult {
    let syscall = Syscall::from_u64(syscall_num)
        .ok_or_else(|| {
            if TRACE_SYSCALLS {
                crate::serial_debugln!("[DBG] invalid syscall: {}", syscall_num);
            }
            SyscallError::InvalidSyscall
        })?;
    
    match syscall {
        Syscall::Send => handle_send(args),
        Syscall::Receive => handle_receive(args),
        Syscall::Reply => handle_reply(args),
        Syscall::Yield => handle_yield(args),
        Syscall::Exit => handle_exit(args),
        Syscall::CreateTask => handle_create_task(args),
        Syscall::MapPage => handle_map_page(args),
        Syscall::UnmapPage => handle_unmap_page(args),
        Syscall::SendZc => handle_send_zc(args),
        Syscall::ReceiveZc => handle_receive_zc(args),
        Syscall::SendFast => handle_send_fast(args),
        Syscall::ReceiveFast => handle_receive_fast(args),
        Syscall::ExecModule => handle_exec_module(args),
        Syscall::ReadScancode => handle_read_scancode(args),
        Syscall::GetFramebufferInfo => handle_get_framebuffer_info(args),
        Syscall::GetTaskInfo => handle_get_task_info(args),
        Syscall::BootfsList => handle_bootfs_list(args),
        Syscall::BootfsRead => handle_bootfs_read(args),
    }
}

/// Handle send syscall
fn handle_send(args: &[u64; 6]) -> SyscallResult {
    let receiver = args[0] as TaskId;
    let msg_type = match args[1] {
        0 => MessageType::Request,
        1 => MessageType::Reply,
        2 => MessageType::Notification,
        3 => MessageType::Interrupt,
        _ => return Err(SyscallError::InvalidArgument),
    };
    
    let sender = crate::microkernel::current_task()
        .ok_or(SyscallError::NoSuchTask)?;
    
    let mut msg = Message::new(sender, receiver, msg_type);
    
    // Copy data from userspace pointer.
    let user_data_addr = args[2];
    let data_len = core::cmp::min(args[3] as usize, 64);
    if data_len > 0 {
        if !is_valid_user_ptr(user_data_addr, data_len) {
            return Err(SyscallError::InvalidArgument);
        }
        let user_data_ptr = user_data_addr as *const u8;
        unsafe {
            core::ptr::copy_nonoverlapping(user_data_ptr, msg.data.as_mut_ptr(), data_len);
        }
        msg.length = data_len;
    }
    
    if data_len <= 32 {
        let words_len = (data_len + 7) / 8;
        let mut words = [0u64; 4];
        for i in 0..data_len {
            words[i / 8] |= (msg.data[i] as u64) << ((i % 8) * 8);
        }
        msg.fast_words[..words_len].copy_from_slice(&words[..words_len]);
        msg.fast_len = words_len;
    }

    crate::ipc::send_message(sender, receiver, &msg).map_err(SyscallError::from)?;
    if TRACE_SYSCALLS {
        crate::serial_debugln!("[DBG] syscall send {} -> {} len {}", sender, receiver, msg.length);
    }
    debug_msg_preview(&msg.data, msg.length);
    
    Ok(0)
}

/// Handle receive syscall
fn handle_receive(args: &[u64; 6]) -> SyscallResult {
    let task_id = crate::microkernel::current_task()
        .ok_or(SyscallError::NoSuchTask)?;
    
    let msg = crate::ipc::receive_message(task_id).map_err(SyscallError::from)?;
    if TRACE_SYSCALLS {
        crate::serial_debugln!("[DBG] syscall recv task {} <- {} len {}", task_id, msg.sender, msg.length);
    }
    debug_msg_preview(&msg.data, msg.length);

    let user_buffer_addr = args[0];
    let buffer_size = args[1] as usize;
    if buffer_size > 0 {
        if !is_valid_user_ptr(user_buffer_addr, buffer_size) {
            return Err(SyscallError::InvalidArgument);
        }
        let user_buffer_ptr = user_buffer_addr as *mut u8;
        let data = msg.data();
        let copy_len = core::cmp::min(data.len(), buffer_size);
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), user_buffer_ptr, copy_len);
            // C-string style terminator for userspace parsers that scan for '\0'.
            if copy_len < buffer_size {
                core::ptr::write(user_buffer_ptr.add(copy_len), 0);
            }
        }
    }
    
    // Return sender ID in low 32 bits and message type in high 32 bits.
    Ok((msg.sender as u64) | ((msg.msg_type as u64) << 32))
}

/// Handle reply syscall
fn handle_reply(args: &[u64; 6]) -> SyscallResult {
    let receiver = args[0] as TaskId;
    
    let sender = crate::microkernel::current_task()
        .ok_or(SyscallError::NoSuchTask)?;
    
    let mut msg = Message::new(sender, receiver, MessageType::Reply);
    
    let user_data_addr = args[1];
    let data_len = core::cmp::min(args[2] as usize, 64);
    if data_len > 0 {
        if !is_valid_user_ptr(user_data_addr, data_len) {
            return Err(SyscallError::InvalidArgument);
        }
        let user_data_ptr = user_data_addr as *const u8;
        unsafe {
            core::ptr::copy_nonoverlapping(user_data_ptr, msg.data.as_mut_ptr(), data_len);
        }
        msg.length = data_len;
    }
    
    crate::ipc::send_message(sender, receiver, &msg).map_err(SyscallError::from)?;
    if TRACE_SYSCALLS {
        crate::serial_debugln!("[DBG] syscall reply {} -> {} len {}", sender, receiver, msg.length);
    }
    debug_msg_preview(&msg.data, msg.length);
    
    Ok(0)
}

/// Handle yield syscall
fn handle_yield(_args: &[u64; 6]) -> SyscallResult {
    crate::microkernel::yield_task();
    Ok(0)
}

/// Handle exit syscall
fn handle_exit(args: &[u64; 6]) -> SyscallResult {
    let exit_code = args[0];
    
    if let Some(task_id) = crate::microkernel::current_task() {
        // Mark task as terminated
        let _ = crate::microkernel::set_task_exit_code(task_id, exit_code);
        let _ = crate::microkernel::set_task_state(task_id, crate::microkernel::TaskState::Terminated);
        
        // Clean up task resources
        crate::microkernel::cleanup_task(task_id);
    }
    
    // Schedule next task
    crate::microkernel::schedule();
    
    Ok(exit_code)
}

/// Handle create task syscall
fn handle_create_task(args: &[u64; 6]) -> SyscallResult {
    let entry_point = args[0];
    let stack_ptr = args[1];
    let address_space = args[2] as u32;
    
    // Create new task
    let task_id = crate::microkernel::create_task(address_space)
        .map_err(|_| SyscallError::NoSuchTask)?;
    
    // Set up task entry point and stack
    crate::microkernel::setup_task(task_id, entry_point, stack_ptr)
        .map_err(|_| SyscallError::InvalidArgument)?;
    
    Ok(task_id as u64)
}

/// Handle map page syscall
fn handle_map_page(args: &[u64; 6]) -> SyscallResult {
    let virtual_addr = args[0];
    let physical_addr = args[1];
    let flags = args[2];
    
    // Get current task's address space
    let task_id = crate::microkernel::current_task()
        .ok_or(SyscallError::NoSuchTask)?;
    
    let task = crate::microkernel::get_task(task_id)
        .ok_or(SyscallError::NoSuchTask)?;
    
    // Map page in task's address space
    crate::mm::map_page(task.address_space, virtual_addr, physical_addr, flags)
        .map_err(|_| SyscallError::PermissionDenied)?;
    
    Ok(0)
}

/// Handle unmap page syscall
fn handle_unmap_page(args: &[u64; 6]) -> SyscallResult {
    let virtual_addr = args[0];
    
    // Get current task's address space
    let task_id = crate::microkernel::current_task()
        .ok_or(SyscallError::NoSuchTask)?;
    
    let task = crate::microkernel::get_task(task_id)
        .ok_or(SyscallError::NoSuchTask)?;
    
    // Unmap page from task's address space
    crate::mm::unmap_page(task.address_space, virtual_addr)
        .map_err(|_| SyscallError::PermissionDenied)?;
    
    Ok(0)
}

/// Handle zero-copy send syscall
fn handle_send_zc(args: &[u64; 6]) -> SyscallResult {
    let receiver = args[0] as TaskId;
    let msg_type = match args[1] {
        0 => MessageType::Request,
        1 => MessageType::Reply,
        2 => MessageType::Notification,
        3 => MessageType::Interrupt,
        _ => return Err(SyscallError::InvalidArgument),
    };
    let sender = crate::microkernel::current_task()
        .ok_or(SyscallError::NoSuchTask)?;

    let user_ptr = args[2];
    let len = args[3] as usize;
    let mut msg = Message::new(sender, receiver, msg_type);
    msg.set_zero_copy(user_ptr, len);

    crate::ipc::send_message(sender, receiver, &msg).map_err(SyscallError::from)?;
    if TRACE_SYSCALLS {
        crate::serial_debugln!("[DBG] syscall send_zc {} -> {} len {}", sender, receiver, len);
    }
    Ok(0)
}

/// Handle zero-copy receive syscall
fn handle_receive_zc(args: &[u64; 6]) -> SyscallResult {
    let task_id = crate::microkernel::current_task()
        .ok_or(SyscallError::NoSuchTask)?;
    let msg = crate::ipc::receive_message(task_id)
        .map_err(SyscallError::from)?;

    let out_ptr_ptr = args[0] as *mut u64;
    let out_len_ptr = args[1] as *mut u64;
    if !out_ptr_ptr.is_null() && !out_len_ptr.is_null() {
        unsafe {
            if msg.zero_copy {
                core::ptr::write(out_ptr_ptr, msg.grant_ptr);
                core::ptr::write(out_len_ptr, msg.grant_len as u64);
            } else {
                core::ptr::write(out_ptr_ptr, 0);
                core::ptr::write(out_len_ptr, 0);
            }
        }
    }

    Ok((msg.sender as u64) | ((msg.msg_type as u64) << 32))
}

fn handle_send_fast(args: &[u64; 6]) -> SyscallResult {
    let receiver = args[0] as TaskId;
    let msg_type = match args[1] {
        0 => MessageType::Request,
        1 => MessageType::Reply,
        2 => MessageType::Notification,
        3 => MessageType::Interrupt,
        _ => return Err(SyscallError::InvalidArgument),
    };
    let sender = crate::microkernel::current_task().ok_or(SyscallError::NoSuchTask)?;

    if fastpath_send_gate(sender, receiver, msg_type) {
        let mut msg = Message::new(sender, receiver, msg_type);
        msg.set_fast_words(&args[2..6]);
        // Fastpath: fixed-size register payload, no user-pointer validation/copy.
        crate::ipc::send_message(sender, receiver, &msg).map_err(SyscallError::from)?;
        return Ok(msg.fast_len as u64);
    }

    // Strict fallback to slowpath semantics.
    let mut packed = [0u8; 32];
    for i in 0..4 {
        let bytes = args[2 + i].to_le_bytes();
        packed[i * 8..(i + 1) * 8].copy_from_slice(&bytes);
    }
    let mut msg = Message::new(sender, receiver, msg_type);
    msg.set_data(&packed);
    crate::ipc::send_message(sender, receiver, &msg).map_err(SyscallError::from)?;
    Ok(4)
}

fn handle_receive_fast(args: &[u64; 6]) -> SyscallResult {
    let out_words = args[0] as *mut u64;
    let max_words = core::cmp::min(args[1] as usize, 4);
    let task_id = crate::microkernel::current_task().ok_or(SyscallError::NoSuchTask)?;
    let msg = crate::ipc::receive_message(task_id).map_err(SyscallError::from)?;
    if !out_words.is_null() && max_words > 0 {
        // Compatibility: if sender used slow send with <=32B payload, expose it
        // through fast receive as packed little-endian words.
        let mut words = [0u64; 4];
        let n = if msg.fast_len > 0 {
            let n = core::cmp::min(msg.fast_len, max_words);
            words[..n].copy_from_slice(&msg.fast_words[..n]);
            n
        } else {
            let bytes = core::cmp::min(msg.length, 32);
            for i in 0..bytes {
                let word = i / 8;
                let shift = (i % 8) * 8;
                words[word] |= (msg.data[i] as u64) << shift;
            }
            core::cmp::min((bytes + 7) / 8, max_words)
        };
        unsafe { core::ptr::copy_nonoverlapping(words.as_ptr(), out_words, n) };
    }
    Ok((msg.sender as u64) | ((msg.msg_type as u64) << 32))
}

fn handle_exec_module(args: &[u64; 6]) -> SyscallResult {
    let name_len = core::cmp::min(args[0] as usize, 40);
    if name_len == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    let mut packed_le = [0u8; 40];
    packed_le[0..8].copy_from_slice(&args[1].to_le_bytes());
    packed_le[8..16].copy_from_slice(&args[2].to_le_bytes());
    packed_le[16..24].copy_from_slice(&args[3].to_le_bytes());
    packed_le[24..32].copy_from_slice(&args[4].to_le_bytes());
    packed_le[32..40].copy_from_slice(&args[5].to_le_bytes());

    let mut packed_be = [0u8; 40];
    packed_be[0..8].copy_from_slice(&args[1].to_be_bytes());
    packed_be[8..16].copy_from_slice(&args[2].to_be_bytes());
    packed_be[16..24].copy_from_slice(&args[3].to_be_bytes());
    packed_be[24..32].copy_from_slice(&args[4].to_be_bytes());
    packed_be[32..40].copy_from_slice(&args[5].to_be_bytes());

    let name = if is_ascii_module_name(&packed_le[..name_len]) {
        &packed_le[..name_len]
    } else if is_ascii_module_name(&packed_be[..name_len]) {
        &packed_be[..name_len]
    } else {
        crate::serial_println!("[KERNEL] syscall exec_module <nonascii:{}>", name_len);
        return Err(SyscallError::InvalidArgument);
    };

    let name_str = core::str::from_utf8(name).unwrap_or("<invalid>");
    crate::serial_println!("[KERNEL] syscall exec_module '{}'", name_str);
    let tid = crate::microkernel::exec_boot_module(name).map_err(|_| SyscallError::NoSuchTask)?;
    crate::serial_println!("[KERNEL] syscall exec_module -> task {}", tid);
    Ok(tid as u64)
}

fn handle_read_scancode(_args: &[u64; 6]) -> SyscallResult {
    if let Some(sc) = crate::arch::x86_64::poll_keyboard_scancode() {
        Ok(sc as u64)
    } else {
        Err(SyscallError::NoMessage)
    }
}

fn handle_get_framebuffer_info(args: &[u64; 6]) -> SyscallResult {
    let fb = crate::arch::x86_64::boot_framebuffer().ok_or(SyscallError::NoMessage)?;
    // out pointers: phys, pitch, width, height, bpp
    let outs = [args[0], args[1], args[2], args[3], args[4]];
    for &p in &outs {
        if p != 0 && !is_valid_user_ptr(p, core::mem::size_of::<u64>()) {
            return Err(SyscallError::InvalidArgument);
        }
    }
    unsafe {
        if args[0] != 0 {
            core::ptr::write(args[0] as *mut u64, fb.phys_addr);
        }
        if args[1] != 0 {
            core::ptr::write(args[1] as *mut u64, fb.pitch as u64);
        }
        if args[2] != 0 {
            core::ptr::write(args[2] as *mut u64, fb.width as u64);
        }
        if args[3] != 0 {
            core::ptr::write(args[3] as *mut u64, fb.height as u64);
        }
        if args[4] != 0 {
            core::ptr::write(args[4] as *mut u64, fb.bpp as u64);
        }
    }
    Ok(0)
}

fn handle_get_task_info(args: &[u64; 6]) -> SyscallResult {
    let task_id = args[0] as TaskId;
    let out_state = args[1];
    let out_exit = args[2];

    if out_state != 0 && !is_valid_user_ptr(out_state, core::mem::size_of::<u32>()) {
        return Err(SyscallError::InvalidArgument);
    }
    if out_exit != 0 && !is_valid_user_ptr(out_exit, core::mem::size_of::<u64>()) {
        return Err(SyscallError::InvalidArgument);
    }

    if let Some((state, exit_code)) = crate::microkernel::get_task_info(task_id) {
        unsafe {
            if out_state != 0 {
                core::ptr::write(out_state as *mut u32, state as u32);
            }
            if out_exit != 0 {
                core::ptr::write(out_exit as *mut u64, exit_code);
            }
        }
        Ok(0)
    } else {
        Err(SyscallError::NoSuchTask)
    }
}

fn handle_bootfs_list(args: &[u64; 6]) -> SyscallResult {
    let out_ptr = args[0];
    let out_len = args[1] as usize;
    if out_len == 0 {
        return Ok(0);
    }
    if !is_valid_user_ptr(out_ptr, out_len) {
        return Err(SyscallError::InvalidArgument);
    }
    let image = crate::microkernel::bootfs_image().ok_or(SyscallError::NoMessage)?;
    let mut off = BOOTFS_MAGIC.len();
    let mut written = 0usize;
    unsafe {
        let out = core::slice::from_raw_parts_mut(out_ptr as *mut u8, out_len);
        while off + 6 <= image.len() {
            let name_len = u16::from_le_bytes([image[off], image[off + 1]]) as usize;
            off += 2;
            let size = u32::from_le_bytes([image[off], image[off + 1], image[off + 2], image[off + 3]]) as usize;
            off += 4;
            if off + name_len > image.len() {
                break;
            }
            let name = &image[off..off + name_len];
            off += name_len;
            if off + size > image.len() {
                break;
            }
            off += size;
            if written > 0 && written < out.len() {
                out[written] = b'\n';
                written += 1;
            }
            let copy = core::cmp::min(name.len(), out.len().saturating_sub(written));
            out[written..written + copy].copy_from_slice(&name[..copy]);
            written += copy;
            if written >= out.len() {
                break;
            }
        }
    }
    Ok(written as u64)
}

fn handle_bootfs_read(args: &[u64; 6]) -> SyscallResult {
    let name_ptr = args[0];
    let name_len = args[1] as usize;
    let out_ptr = args[2];
    let out_len = args[3] as usize;
    if name_len == 0 || out_len == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    if !is_valid_user_ptr(name_ptr, name_len) || !is_valid_user_ptr(out_ptr, out_len) {
        return Err(SyscallError::InvalidArgument);
    }
    let name = unsafe { core::slice::from_raw_parts(name_ptr as *const u8, name_len) };
    let image = crate::microkernel::bootfs_image().ok_or(SyscallError::NoMessage)?;
    let mut off = BOOTFS_MAGIC.len();
    while off + 6 <= image.len() {
        let nlen = u16::from_le_bytes([image[off], image[off + 1]]) as usize;
        off += 2;
        let size = u32::from_le_bytes([image[off], image[off + 1], image[off + 2], image[off + 3]]) as usize;
        off += 4;
        if off + nlen > image.len() {
            break;
        }
        let entry_name = &image[off..off + nlen];
        off += nlen;
        if off + size > image.len() {
            break;
        }
        let data = &image[off..off + size];
        off += size;
        if entry_name == name {
            let copy = core::cmp::min(out_len, data.len());
            unsafe {
                let out = core::slice::from_raw_parts_mut(out_ptr as *mut u8, out_len);
                out[..copy].copy_from_slice(&data[..copy]);
            }
            return Ok(copy as u64);
        }
    }
    Err(SyscallError::NoMessage)
}

#[inline]
fn fastpath_send_gate(sender: TaskId, receiver: TaskId, msg_type: MessageType) -> bool {
    // Hard gate: reject edge cases and fall back to slowpath.
    if receiver >= MAX_TASKS as TaskId || sender == 0 || receiver == 0 {
        return false;
    }
    if matches!(msg_type, MessageType::Interrupt) {
        return false;
    }
    let recv_task = match crate::microkernel::get_task(receiver) {
        Some(t) => t,
        None => return false,
    };
    if !matches!(
        recv_task.state,
        crate::microkernel::TaskState::Receiving | crate::microkernel::TaskState::Blocked
    ) {
        return false;
    }
    true
}

fn is_ascii_module_name(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    for &b in data {
        let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'/' || b == b'.';
        if !ok {
            return false;
        }
    }
    true
}
