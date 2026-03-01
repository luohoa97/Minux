//! Boot-related functionality
//! 
//! Supports both BIOS (multiboot2) and UEFI boot

pub mod multiboot_header;
pub mod multiboot;
pub mod efi;

pub use multiboot::BootModule;
pub use multiboot::FramebufferInfo;

/// Get boot modules (works for both multiboot2 and EFI)
pub fn get_boot_modules(boot_info: usize, protocol: BootProtocol) -> &'static [BootModule] {
    match protocol {
        BootProtocol::Multiboot2 => {
            crate::serial_println!("[BOOT] Using Multiboot2");
            multiboot::get_boot_modules(boot_info)
        }
        BootProtocol::Efi => {
            crate::serial_println!("[BOOT] Using UEFI");
            efi::init(boot_info);
            efi::get_efi_modules()
        }
        BootProtocol::Unknown => {
            crate::serial_println!("[BOOT] Unknown boot protocol");
            &[]
        }
    }
}

pub fn get_boot_framebuffer(boot_info: usize, protocol: BootProtocol) -> Option<FramebufferInfo> {
    match protocol {
        BootProtocol::Multiboot2 => multiboot::get_framebuffer_info(boot_info),
        BootProtocol::Efi | BootProtocol::Unknown => None,
    }
}

/// Boot protocol detection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootProtocol {
    Multiboot2,
    Efi,
    Unknown,
}

/// Detect which boot protocol was used
pub fn detect_boot_protocol(boot_info: usize) -> BootProtocol {
    // Multiboot2 hands us a pointer to the info block (magic is in EAX, not in the block).
    if multiboot::looks_like_multiboot2(boot_info) {
        return BootProtocol::Multiboot2;
    }
    
    // Check for EFI signature (fallback)
    // EFI system table has signature at offset 0
    if boot_info != 0 {
        let signature = unsafe { *(boot_info as *const u64) };
        if signature == 0x5453595320494249 { // "IBI SYST"
            return BootProtocol::Efi;
        }
    }
    
    BootProtocol::Unknown
}
