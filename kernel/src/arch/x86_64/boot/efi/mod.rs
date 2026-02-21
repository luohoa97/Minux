//! UEFI boot support

mod system_table;
mod boot_services;
mod protocols;

pub use system_table::*;
pub use boot_services::*;
pub use protocols::*;

use super::BootModule;

/// EFI boot information
static mut EFI_BOOT_INFO: Option<EfiBootInfo> = None;

#[derive(Debug)]
pub struct EfiBootInfo {
    pub system_table: *const SystemTable,
    pub modules: [Option<BootModule>; 16],
    pub module_count: usize,
}

/// Initialize EFI boot
pub fn init(system_table_ptr: usize) {
    let system_table = unsafe { &*(system_table_ptr as *const SystemTable) };
    
    crate::serial_println!("[EFI] System Table at: 0x{:x}", system_table_ptr);
    crate::serial_println!("[EFI] Firmware Vendor: {:?}", system_table.firmware_vendor);
    crate::serial_println!("[EFI] Revision: 0x{:x}", system_table.header.revision);
    
    unsafe {
        EFI_BOOT_INFO = Some(EfiBootInfo {
            system_table: system_table_ptr as *const SystemTable,
            modules: [None; 16],
            module_count: 0,
        });
    }
}

/// Get EFI boot modules
pub fn get_efi_modules() -> &'static [BootModule] {
    unsafe {
        if let Some(ref info) = EFI_BOOT_INFO {
            let valid_modules: &[Option<BootModule>] = &info.modules[..info.module_count];
            // Filter out None values and return slice
            // For now return empty until we parse loaded image protocol
            &[]
        } else {
            &[]
        }
    }
}
