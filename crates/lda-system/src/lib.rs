//! Read-only views of the local Linux system.
//!
//! Collectors prefer `/proc`, `/sys`, and structured command output. They do
//! not start, stop, or reconfigure anything.

#![forbid(unsafe_code)]

mod command;
mod desktop;
mod live;
mod parse;

pub use desktop::{classify_desktop, DesktopInfo, DesktopProbe};
pub use live::{
    battery_info, boot_info, cpu_info, gpu_info, hardware_devices, kernel_info, memory_info,
    network_info, package_info, process_info, service_status, storage_info, system_info,
    validate_package_name, validate_unit_name,
};
pub use parse::{parse_cpuinfo, parse_meminfo, parse_os_release};
