//! Inter-Process Communication for minux microkernel
//!
//! IPC is the heart of a microkernel. All services (drivers, filesystems, etc.)
//! communicate through message passing. This provides:
//! - Synchronous and asynchronous message passing
//! - Capability-based security
//! - Memory protection between address spaces

mod message;
mod endpoint;
mod syscall;

pub use message::*;
pub use endpoint::MessageQueue;
pub use syscall::*;

use crate::microkernel::TaskId;

/// Initialize IPC subsystem
pub fn init() {
    endpoint::init();
}

/// Process pending IPC messages (called from main loop)
pub fn process_messages() {
    endpoint::process_pending_messages();
}

/// Send message (blocking)
pub fn send_message(sender: TaskId, receiver: TaskId, msg: &Message) -> Result<(), IpcError> {
    endpoint::send_message(sender, receiver, msg)
}

/// Receive message (blocking)
pub fn receive_message(task_id: TaskId) -> Result<Message, IpcError> {
    endpoint::receive_message(task_id)
}

/// Try to receive message (non-blocking)
pub fn try_receive_message(task_id: TaskId) -> Option<Message> {
    endpoint::try_receive_message(task_id)
}

pub fn clear_task_queue(task_id: TaskId) {
    endpoint::clear_task_queue(task_id)
}

/// IPC error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    InvalidTask,
    QueueFull,
    NoMessage,
    PermissionDenied,
    InvalidCapability,
}
