//! System call interface for IPC

use super::{Message, MessageType, IpcError};
use crate::microkernel::TaskId;

/// System call numbers for IPC
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum IpcSyscall {
    Send = 1,
    Receive = 2,
    Reply = 3,
    Notify = 4,
}

/// System call handler for IPC operations
pub fn handle_ipc_syscall(syscall: IpcSyscall, args: &[u64]) -> Result<u64, IpcError> {
    match syscall {
        IpcSyscall::Send => {
            if args.len() < 3 {
                return Err(IpcError::InvalidTask);
            }
            
            let sender = crate::microkernel::current_task().ok_or(IpcError::InvalidTask)?;
            let receiver = args[0] as TaskId;
            let msg_type = match args[1] {
                0 => MessageType::Request,
                1 => MessageType::Reply,
                2 => MessageType::Notification,
                3 => MessageType::Interrupt,
                _ => return Err(IpcError::InvalidTask),
            };
            
            let mut msg = Message::new(sender, receiver, msg_type);
            
            // Copy data from userspace memory
            let user_data_ptr = args[2] as *const u8;
            let data_len = core::cmp::min(args[3] as usize, 64);
            
            if data_len > 0 && !user_data_ptr.is_null() {
                let mut data = [0u8; 64];
                unsafe {
                    // Copy from userspace (with bounds checking)
                    core::ptr::copy_nonoverlapping(user_data_ptr, data.as_mut_ptr(), data_len);
                }
                msg.set_data(&data[..data_len]);
            }
            
            super::send_message(sender, receiver, &msg)?;
            Ok(0)
        }
        
        IpcSyscall::Receive => {
            let task_id = crate::microkernel::current_task().ok_or(IpcError::InvalidTask)?;
            let msg = super::receive_message(task_id)?;
            
            // Copy message data to userspace buffer
            let user_buffer_ptr = args[0] as *mut u8;
            let buffer_size = args[1] as usize;
            
            if !user_buffer_ptr.is_null() && buffer_size > 0 {
                let data = msg.data();
                let copy_len = core::cmp::min(data.len(), buffer_size);
                
                unsafe {
                    // Copy message data to userspace
                    core::ptr::copy_nonoverlapping(data.as_ptr(), user_buffer_ptr, copy_len);
                }
            }
            
            // Return message info: sender ID in lower 32 bits, message type in upper 32 bits
            Ok(msg.sender as u64 | ((msg.msg_type as u64) << 32))
        }
        
        IpcSyscall::Reply => {
            // Similar to send, but specifically for replies
            if args.len() < 2 {
                return Err(IpcError::InvalidTask);
            }
            
            let sender = crate::microkernel::current_task().ok_or(IpcError::InvalidTask)?;
            let receiver = args[0] as TaskId;
            
            let mut msg = Message::new(sender, receiver, MessageType::Reply);
            let data = args[1].to_le_bytes();
            msg.set_data(&data);
            
            super::send_message(sender, receiver, &msg)?;
            Ok(0)
        }
        
        IpcSyscall::Notify => {
            // Asynchronous notification
            if args.len() < 1 {
                return Err(IpcError::InvalidTask);
            }
            
            let sender = crate::microkernel::current_task().ok_or(IpcError::InvalidTask)?;
            let receiver = args[0] as TaskId;
            
            let msg = Message::new(sender, receiver, MessageType::Notification);
            super::send_message(sender, receiver, &msg)?;
            Ok(0)
        }
    }
}
