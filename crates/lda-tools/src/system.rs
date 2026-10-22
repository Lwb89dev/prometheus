use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lda_common::{ConfirmationPolicy, PermissionLevel, Result, Risk};
use lda_system::{
    battery_info, boot_info, cpu_info, gpu_info, hardware_devices, kernel_info, memory_info,
    network_info, package_info, process_info, service_status, storage_info, system_info,
};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::registry::{Tool, ToolRegistry, ToolSpec};

struct SysTool {
    spec: ToolSpec,
    kind: Kind,
}

#[derive(Clone, Copy)]
enum Kind {
    System,
    Cpu,
    Memory,
    Gpu,
    Storage,
    Battery,
    Network,
    Kernel,
    Boot,
    Service,
    Process,
    Package,
    Hardware,
}

#[async_trait]
impl Tool for SysTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    async fn execute(&self, arguments: &Value, _cancel: &CancellationToken) -> Result<Value> {
        let kind = self.kind;
        let arguments = arguments.clone();
        tokio::task::spawn_blocking(move || run(kind, &arguments))
            .await
            .map_err(|err| lda_common::Error::Other(err.to_string()))?
    }
}

fn run(kind: Kind, arguments: &Value) -> Result<Value> {
    match kind {
        Kind::System => Ok(system_info()),
        Kind::Cpu => Ok(cpu_info()),
        Kind::Memory => Ok(memory_info()),
        Kind::Gpu => Ok(gpu_info()),
        Kind::Storage => Ok(storage_info()),
        Kind::Battery => Ok(battery_info()),
        Kind::Network => Ok(network_info()),
        Kind::Kernel => Ok(kernel_info()),
        Kind::Boot => Ok(boot_info()),
        Kind::Hardware => Ok(hardware_devices()),
        Kind::Process => {
            let pid = arguments
                .get("pid")
                .and_then(Value::as_u64)
                .map(|n| n as u32);
            process_info(pid)
        }
        Kind::Package => {
            let name = arguments.get("name").and_then(Value::as_str);
            package_info(name)
        }
        Kind::Service => {
            let scope = arguments
                .get("scope")
                .and_then(Value::as_str)
                .unwrap_or("system");
            let name = arguments.get("name").and_then(Value::as_str);
            service_status(scope, name)
        }
    }
}

pub fn system_registry() -> ToolRegistry {
    ToolRegistry::new(vec![
        tool(Kind::System, "get_system_info", "Read distribution, architecture, desktop session, kernel release, and a memory summary.", empty_object()),
        tool(Kind::Cpu, "get_cpu_info", "Read CPU model, count, virtualization flags, and the current load average. Load average is not a utilization percentage.", empty_object()),
        tool(Kind::Memory, "get_memory_info", "Read MemTotal, MemAvailable, and swap figures from /proc/meminfo.", empty_object()),
        tool(Kind::Gpu, "get_gpu_info", "Read DRM card vendor, device, and driver from sysfs. This does not diagnose a GPU failure.", empty_object()),
        tool(Kind::Storage, "get_storage_info", "Read mounted filesystems and free space. This does not delete or unmount anything.", empty_object()),
        tool(Kind::Battery, "get_battery_info", "Read battery firmware reports from sysfs. A strange reading is not by itself a confirmed hardware failure.", empty_object()),
        tool(Kind::Network, "get_network_info", "Read interface state, the IPv4 default route, and local addresses. Includes MAC addresses.", empty_object()),
        tool(Kind::Kernel, "get_kernel_info", "Read the running kernel release and version string.", empty_object()),
        tool(Kind::Boot, "get_boot_info", "Read uptime and boot time.", empty_object()),
        tool(Kind::Hardware, "inspect_hardware_devices", "Read a bounded PCI device list and non-serial DMI fields. Serial numbers are not collected.", empty_object()),
        tool(Kind::Process, "get_process_info", "Count processes, or read Name, State, and RSS for one pid. Does not read environ or command lines.", json!({"type":"object","additionalProperties":false,"properties":{"pid":{"type":"integer","minimum":1,"maximum":4194304}}})),
        tool(Kind::Package, "get_package_info", "Detect package managers, or query one package name. Does not install or remove packages.", json!({"type":"object","additionalProperties":false,"properties":{"name":{"type":"string","maxLength":128}}})),
        tool(Kind::Service, "get_service_status", "Read one systemd unit. Does not start or stop it. scope is user or system.", json!({"type":"object","additionalProperties":false,"properties":{"scope":{"type":"string","enum":["user","system"]},"name":{"type":"string","maxLength":256}}})),
    ])
}

fn tool(
    kind: Kind,
    name: &'static str,
    description: &'static str,
    parameters: Value,
) -> Arc<dyn Tool> {
    Arc::new(SysTool {
        kind,
        spec: ToolSpec {
            name,
            description,
            parameters,
            permission: PermissionLevel::ReadOnly,
            risk: Risk::Low,
            timeout: Duration::from_secs(15),
            confirmation: ConfirmationPolicy::Never,
        },
    })
}

fn empty_object() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}
