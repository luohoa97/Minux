//! Minux userspace system call library

#![no_std]

/// Panic handler for userspace programs
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // In userspace, we can call exit syscall
    syscall::exit(1);
}

/// System call numbers
#[repr(u64)]
pub enum Syscall {
    Send = 1,
    Receive = 2,
    Reply = 3,
    Yield = 4,
    Exit = 5,
    CreateTask = 6,
    MapPage = 7,
    UnmapPage = 8,
    SendZc = 9,
    ReceiveZc = 10,
    SendFast = 11,
    ReceiveFast = 12,
    ExecModule = 13,
    ReadScancode = 14,
    GetFramebufferInfo = 15,
    GetTaskInfo = 16,
    BootfsList = 17,
    BootfsRead = 18,
}

#[derive(Clone, Copy, Debug)]
pub struct FramebufferInfo {
    pub phys_addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
}

/// Message types
#[repr(u32)]
pub enum MessageType {
    Request = 0,
    Reply = 1,
    Notification = 2,
    Interrupt = 3,
}

/// Task ID type
pub type TaskId = u32;

/// System call interface
pub mod syscall {
    use super::*;
    
    /// Perform system call
    #[inline]
    pub unsafe fn syscall6(
        syscall: Syscall,
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> u64 {
        let result: u64;
        unsafe {
            core::arch::asm!(
                "int 0x80",
                in("rax") syscall as u64,
                in("rdi") arg1,
                in("rsi") arg2,
                in("rdx") arg3,
                in("r10") arg4,
                in("r8") arg5,
                in("r9") arg6,
                lateout("rax") result,
                options(nostack, preserves_flags)
            );
        }
        result
    }
    
    /// Send IPC message
    pub fn send_message(receiver: TaskId, msg_type: MessageType, data: &[u8]) -> Result<(), ()> {
        let result = unsafe {
            syscall6(
                Syscall::Send,
                receiver as u64,
                msg_type as u64,
                data.as_ptr() as u64,
                data.len() as u64,
                0,
                0,
            )
        };
        
        if result == u64::MAX {
            Err(())
        } else {
            Ok(())
        }
    }

    /// Send IPC message using zero-copy descriptor passing.
    /// The sender must keep `data` alive and unchanged until receiver consumes it.
    pub fn send_message_zc(receiver: TaskId, msg_type: MessageType, data: &[u8]) -> Result<(), ()> {
        let result = unsafe {
            syscall6(
                Syscall::SendZc,
                receiver as u64,
                msg_type as u64,
                data.as_ptr() as u64,
                data.len() as u64,
                0,
                0,
            )
        };

        if result == u64::MAX {
            Err(())
        } else {
            Ok(())
        }
    }
    
    /// Receive IPC message
    pub fn receive_message(buffer: &mut [u8]) -> Result<(TaskId, MessageType), ()> {
        let result = unsafe {
            syscall6(
                Syscall::Receive,
                buffer.as_mut_ptr() as u64,
                buffer.len() as u64,
                0,
                0,
                0,
                0,
            )
        };
        
        if result == u64::MAX {
            Err(())
        } else {
            let sender = (result & 0xFFFFFFFF) as TaskId;
            let msg_type = match (result >> 32) & 0xFFFFFFFF {
                0 => MessageType::Request,
                1 => MessageType::Reply,
                2 => MessageType::Notification,
                3 => MessageType::Interrupt,
                _ => return Err(()),
            };
            Ok((sender, msg_type))
        }
    }

    /// Receive IPC message descriptor for zero-copy transfer.
    /// Returns `(sender, msg_type, ptr, len)`.
    /// If `ptr` is null or `len` is zero, payload was not provided as zero-copy.
    pub fn receive_message_zc() -> Result<(TaskId, MessageType, *const u8, usize), ()> {
        let mut ptr: u64 = 0;
        let mut len: u64 = 0;
        let result = unsafe {
            syscall6(
                Syscall::ReceiveZc,
                (&mut ptr as *mut u64) as u64,
                (&mut len as *mut u64) as u64,
                0,
                0,
                0,
                0,
            )
        };

        if result == u64::MAX {
            return Err(());
        }
        let sender = (result & 0xFFFFFFFF) as TaskId;
        let msg_type = match (result >> 32) & 0xFFFFFFFF {
            0 => MessageType::Request,
            1 => MessageType::Reply,
            2 => MessageType::Notification,
            3 => MessageType::Interrupt,
            _ => return Err(()),
        };
        Ok((sender, msg_type, ptr as *const u8, len as usize))
    }

    /// Fast IPC send: pass 4 machine words directly in registers.
    pub fn send_message_fast(receiver: TaskId, msg_type: MessageType, words: [u64; 4]) -> Result<usize, ()> {
        let result = unsafe {
            syscall6(
                Syscall::SendFast,
                receiver as u64,
                msg_type as u64,
                words[0],
                words[1],
                words[2],
                words[3],
            )
        };
        if result == u64::MAX { Err(()) } else { Ok(result as usize) }
    }

    /// Fast IPC receive into machine words.
    pub fn receive_message_fast(words: &mut [u64; 4]) -> Result<(TaskId, MessageType), ()> {
        let result = unsafe {
            syscall6(
                Syscall::ReceiveFast,
                words.as_mut_ptr() as u64,
                words.len() as u64,
                0,
                0,
                0,
                0,
            )
        };
        if result == u64::MAX {
            return Err(());
        }
        let sender = (result & 0xFFFFFFFF) as TaskId;
        let msg_type = match (result >> 32) & 0xFFFFFFFF {
            0 => MessageType::Request,
            1 => MessageType::Reply,
            2 => MessageType::Notification,
            3 => MessageType::Interrupt,
            _ => return Err(()),
        };
        Ok((sender, msg_type))
    }
    
    /// Reply to IPC message
    pub fn reply_message(receiver: TaskId, data: &[u8]) -> Result<(), ()> {
        let result = unsafe {
            syscall6(
                Syscall::Reply,
                receiver as u64,
                data.as_ptr() as u64,
                data.len() as u64,
                0,
                0,
                0,
            )
        };
        
        if result == u64::MAX {
            Err(())
        } else {
            Ok(())
        }
    }
    
    /// Yield CPU to scheduler
    pub fn yield_cpu() {
        unsafe {
            syscall6(Syscall::Yield, 0, 0, 0, 0, 0, 0);
        }
    }
    
    /// Exit current task
    pub fn exit(code: u64) -> ! {
        unsafe {
            syscall6(Syscall::Exit, code, 0, 0, 0, 0, 0);
        }
        loop {}
    }
    
    /// Map memory page
    pub fn map_page(virtual_addr: u64, physical_addr: u64, flags: u64) -> Result<(), ()> {
        let result = unsafe {
            syscall6(
                Syscall::MapPage,
                virtual_addr,
                physical_addr,
                flags,
                0,
                0,
                0,
            )
        };
        
        if result == u64::MAX {
            Err(())
        } else {
            Ok(())
        }
    }

    /// Ask kernel to execute a deferred boot module by name.
    pub fn exec_module(name: &[u8]) -> Result<TaskId, ()> {
        // Pass module name by value in registers to avoid pointer ownership/lifetime issues.
        let n = core::cmp::min(name.len(), 40);
        if n == 0 {
            return Err(());
        }
        let mut packed = [0u8; 40];
        packed[..n].copy_from_slice(&name[..n]);
        let w0 = u64::from_le_bytes(packed[0..8].try_into().unwrap());
        let w1 = u64::from_le_bytes(packed[8..16].try_into().unwrap());
        let w2 = u64::from_le_bytes(packed[16..24].try_into().unwrap());
        let w3 = u64::from_le_bytes(packed[24..32].try_into().unwrap());
        let w4 = u64::from_le_bytes(packed[32..40].try_into().unwrap());
        let result = unsafe {
            syscall6(
                Syscall::ExecModule,
                n as u64,
                w0,
                w1,
                w2,
                w3,
                w4,
            )
        };
        if result == u64::MAX {
            Err(())
        } else {
            Ok(result as TaskId)
        }
    }

    /// Poll one raw PS/2 set1 scancode from kernel IRQ queue.
    pub fn read_scancode() -> Option<u8> {
        let result = unsafe { syscall6(Syscall::ReadScancode, 0, 0, 0, 0, 0, 0) };
        if result == u64::MAX {
            None
        } else {
            Some((result & 0xff) as u8)
        }
    }

    pub fn get_framebuffer_info() -> Option<FramebufferInfo> {
        let mut phys = 0u64;
        let mut pitch = 0u64;
        let mut width = 0u64;
        let mut height = 0u64;
        let mut bpp = 0u64;
        let result = unsafe {
            syscall6(
                Syscall::GetFramebufferInfo,
                (&mut phys as *mut u64) as u64,
                (&mut pitch as *mut u64) as u64,
                (&mut width as *mut u64) as u64,
                (&mut height as *mut u64) as u64,
                (&mut bpp as *mut u64) as u64,
                0,
            )
        };
        if result == u64::MAX {
            None
        } else {
            Some(FramebufferInfo {
                phys_addr: phys,
                pitch: pitch as u32,
                width: width as u32,
                height: height as u32,
                bpp: bpp as u8,
            })
        }
    }

    pub fn get_task_info(task_id: TaskId) -> Option<(u32, u64)> {
        let mut state: u32 = 0;
        let mut exit_code: u64 = 0;
        let result = unsafe {
            syscall6(
                Syscall::GetTaskInfo,
                task_id as u64,
                (&mut state as *mut u32) as u64,
                (&mut exit_code as *mut u64) as u64,
                0,
                0,
                0,
            )
        };
        if result == u64::MAX {
            None
        } else {
            Some((state, exit_code))
        }
    }

    pub fn bootfs_list(out: &mut [u8]) -> Option<usize> {
        let result = unsafe {
            syscall6(
                Syscall::BootfsList,
                out.as_mut_ptr() as u64,
                out.len() as u64,
                0,
                0,
                0,
                0,
            )
        };
        if result == u64::MAX {
            None
        } else {
            Some(result as usize)
        }
    }

    pub fn bootfs_read(name: &[u8], out: &mut [u8]) -> Option<usize> {
        let result = unsafe {
            syscall6(
                Syscall::BootfsRead,
                name.as_ptr() as u64,
                name.len() as u64,
                out.as_mut_ptr() as u64,
                out.len() as u64,
                0,
                0,
            )
        };
        if result == u64::MAX {
            None
        } else {
            Some(result as usize)
        }
    }
}

/// VGA text mode interface
pub mod vga {
    /// VGA text buffer
    pub const VGA_BUFFER: *mut u8 = 0xb8000 as *mut u8;
    pub const VGA_WIDTH: usize = 80;
    pub const VGA_HEIGHT: usize = 25;
    
    /// VGA colors
    #[repr(u8)]
    #[derive(Clone, Copy)]
    pub enum Color {
        Black = 0,
        Blue = 1,
        Green = 2,
        Cyan = 3,
        Red = 4,
        Magenta = 5,
        Brown = 6,
        LightGray = 7,
        DarkGray = 8,
        LightBlue = 9,
        LightGreen = 10,
        LightCyan = 11,
        LightRed = 12,
        Pink = 13,
        Yellow = 14,
        White = 15,
    }
    
    /// Write character to VGA buffer
    pub unsafe fn write_char(x: usize, y: usize, ch: u8, fg: Color, bg: Color) {
        if x < VGA_WIDTH && y < VGA_HEIGHT {
            let offset = (y * VGA_WIDTH + x) * 2;
            let color = (bg as u8) << 4 | (fg as u8);
            
            unsafe {
                *VGA_BUFFER.add(offset) = ch;
                *VGA_BUFFER.add(offset + 1) = color;
            }
        }
    }
    
    /// Write string to VGA buffer
    pub unsafe fn write_string(x: usize, y: usize, s: &str, fg: Color, bg: Color) {
        for (i, ch) in s.bytes().enumerate() {
            if x + i >= VGA_WIDTH {
                break;
            }
            unsafe {
                write_char(x + i, y, ch, fg, bg);
            }
        }
    }
    
    /// Clear screen
    pub unsafe fn clear_screen(bg: Color) {
        for y in 0..VGA_HEIGHT {
            for x in 0..VGA_WIDTH {
                unsafe {
                    write_char(x, y, b' ', Color::White, bg);
                }
            }
        }
    }
}
