//! Inter-Process Communication for minux microkernel

use crate::process::Pid;

/// Message types for IPC
#[derive(Debug, Clone, Copy)]
pub enum MessageType {
    Request = 0,
    Response = 1,
    Notification = 2,
}

/// IPC message structure
#[derive(Debug, Clone, Copy)]
pub struct Message {
    pub sender: Pid,
    pub receiver: Pid,
    pub msg_type: MessageType,
    pub data: [u8; 64], // Small fixed-size message
}

/// Message queue for IPC
pub struct MessageQueue {
    messages: [Option<Message>; 16], // Small fixed queue
    head: usize,
    tail: usize,
    count: usize,
}

impl MessageQueue {
    /// Create new message queue
    pub const fn new() -> Self {
        Self {
            messages: [None; 16],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    /// Send message
    pub fn send(&mut self, message: Message) -> Result<(), ()> {
        if self.count >= 16 {
            return Err(()); // Queue full
        }

        self.messages[self.tail] = Some(message);
        self.tail = (self.tail + 1) % 16;
        self.count += 1;
        Ok(())
    }

    /// Receive message
    pub fn receive(&mut self) -> Option<Message> {
        if self.count == 0 {
            return None;
        }

        let message = self.messages[self.head].take();
        self.head = (self.head + 1) % 16;
        self.count -= 1;
        message
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Global message queue
static MESSAGE_QUEUE: spin::Mutex<MessageQueue> = spin::Mutex::new(MessageQueue::new());

/// Initialize IPC
pub fn init() {
    // IPC initialization
}

/// Send IPC message
pub fn send_message(sender: Pid, receiver: Pid, msg_type: MessageType, data: &[u8]) -> Result<(), ()> {
    let mut message_data = [0u8; 64];
    let len = core::cmp::min(data.len(), 64);
    message_data[..len].copy_from_slice(&data[..len]);

    let message = Message {
        sender,
        receiver,
        msg_type,
        data: message_data,
    };

    MESSAGE_QUEUE.lock().send(message)
}

/// Receive IPC message
pub fn receive_message() -> Option<Message> {
    MESSAGE_QUEUE.lock().receive()
}