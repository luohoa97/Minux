//! System call interrupt handler for x86_64

use core::arch::global_asm;
use core::sync::atomic::{AtomicU64, Ordering};

static SYSCALL_COUNT: AtomicU64 = AtomicU64::new(0);

global_asm!(
    r#"
    .global syscall_entry_asm
syscall_entry_asm:
    # Save GPRs
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    # SysV call args for syscall_dispatch:
    # rdi=sysno(rax), rsi=a1(rdi), rdx=a2(rsi), rcx=a3(rdx), r8=a4(r10), r9=a5(r8), stack=a6(r9)
    mov rdi, [rsp + 112]
    mov rsi, [rsp + 72]
    mov rdx, [rsp + 80]
    mov rcx, [rsp + 88]
    mov r8,  [rsp + 40]
    mov r9,  [rsp + 56]
    mov rax, [rsp + 48]
    push rax
    call syscall_dispatch
    add rsp, 8

    # Restore registers except original saved RAX (skip it so return value in RAX survives)
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    add rsp, 8
    iretq
"#
);

unsafe extern "C" {
    pub fn syscall_entry_asm();
}

#[unsafe(no_mangle)]
extern "C" fn syscall_dispatch(
    syscall_num: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> u64 {
    let n = SYSCALL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if n <= 64 {
        crate::serial_debugln!("[DBG] syscall #{} num={}", n, syscall_num);
    }
    let args = [arg1, arg2, arg3, arg4, arg5, arg6];
    match crate::microkernel::handle_syscall(syscall_num, &args) {
        Ok(v) => v,
        Err(_) => u64::MAX,
    }
}
