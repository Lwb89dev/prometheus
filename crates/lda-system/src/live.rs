use std::path::Path;
use std::time::Duration;

use lda_common::{now_ms, Error, Result};
use serde_json::{json, Map, Value};

use crate::command::{find_program, run_argv};
use crate::desktop::{classify_desktop, DesktopProbe};
use crate::parse::{
    parse_btime, parse_cpuinfo, parse_ipv4_route, parse_meminfo, parse_mounts, parse_os_release,
    parse_uptime,
};

const OBSERVATION: &str = "observation";

pub fn system_info() -> Value {
    let os = read_text("/etc/os-release")
        .map(|text| parse_os_release(&text))
        .unwrap_or_default();
    let desktop = classify_desktop(&DesktopProbe::from_env());
    let kernel = read_text("/proc/sys/kernel/osrelease")
        .unwrap_or_default()
        .trim()
        .to_string();
    let memory = read_text("/proc/meminfo").map(|text| parse_meminfo(&text));
    json!({
        "kind": OBSERVATION,
        "observed_at_ms": now_ms(),
        "os": os,
        "arch": std::env::consts::ARCH,
        "desktop": desktop,
        "kernel_release": kernel,
        "hostname": read_text("/proc/sys/kernel/hostname").unwrap_or_default().trim(),
        "memory": memory,
        "note": "Desktop and session fields come from the daemon environment. A service started outside the graphical session may report them as unknown.",
    })
}

pub fn cpu_info() -> Value {
    let text = read_text("/proc/cpuinfo").unwrap_or_default();
    let info = parse_cpuinfo(&text);
    let load = read_text("/proc/loadavg").unwrap_or_default();
    json!({
        "kind": OBSERVATION,
        "source": "/proc/cpuinfo",
        "cpu": info,
        "loadavg": load.trim(),
        "note": "A single loadavg reading is not a CPU utilization percentage.",
    })
}

pub fn memory_info() -> Value {
    let text = read_text("/proc/meminfo").unwrap_or_default();
    json!({
        "kind": OBSERVATION,
        "memory": parse_meminfo(&text),
        "note": "Values are kernel reports in KiB.",
    })
}

pub fn kernel_info() -> Value {
    json!({
        "kind": OBSERVATION,
        "osrelease": read_text("/proc/sys/kernel/osrelease").unwrap_or_default().trim(),
        "os_type": read_text("/proc/sys/kernel/ostype").unwrap_or_default().trim(),
        "version": clip_owned(read_text("/proc/version").unwrap_or_default().trim(), 400),
        "arch": std::env::consts::ARCH,
    })
}

pub fn boot_info() -> Value {
    let uptime = read_text("/proc/uptime").and_then(|text| parse_uptime(&text));
    let btime = read_text("/proc/stat").and_then(|text| parse_btime(&text));
    json!({
        "kind": OBSERVATION,
        "uptime_seconds": uptime,
        "boot_time_unix": btime,
        "source": ["/proc/uptime", "/proc/stat"],
    })
}

pub fn storage_info() -> Value {
    let text = read_text("/proc/mounts").unwrap_or_default();
    let mut seen = Vec::new();
    let mut mounts = Vec::new();
    for mount in parse_mounts(&text) {
        if seen.iter().any(|target: &String| target == &mount.target) {
            continue;
        }
        seen.push(mount.target.clone());
        let usage = disk_usage(Path::new(&mount.target));
        mounts.push(json!({
            "source": mount.source,
            "target": mount.target,
            "fstype": mount.fstype,
            "total_bytes": usage.map(|u| u.0),
            "available_bytes": usage.map(|u| u.1),
        }));
    }
    json!({
        "kind": OBSERVATION,
        "source": "/proc/mounts",
        "mounts": mounts,
    })
}

pub fn gpu_info() -> Value {
    let mut devices = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return json!({"kind": OBSERVATION, "available": false, "devices": [], "reason": "sysfs drm is not readable"});
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_drm_card(&name) {
            continue;
        }
        let device = entry.path().join("device");
        devices.push(json!({
            "card": name,
            "vendor": sys_trim(&device.join("vendor")),
            "device": sys_trim(&device.join("device")),
            "driver": driver_name(&device),
        }));
        if devices.len() == 16 {
            break;
        }
    }
    json!({
        "kind": OBSERVATION,
        "source": "/sys/class/drm",
        "devices": devices,
        "note": "PCI ids and driver names are not a performance or failure diagnosis.",
    })
}

pub fn battery_info() -> Value {
    let mut batteries = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
        return json!({
            "kind": OBSERVATION,
            "available": false,
            "batteries": [],
            "note": "No power_supply sysfs. This host may be a desktop without a battery.",
        });
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let kind = sys_trim(&path.join("type")).unwrap_or_default();
        if kind != "Battery" {
            continue;
        }
        batteries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "status": sys_trim(&path.join("status")),
            "capacity_percent": sys_trim(&path.join("capacity")),
            "energy_now": sys_trim(&path.join("energy_now")),
            "energy_full": sys_trim(&path.join("energy_full")),
            "voltage_now": sys_trim(&path.join("voltage_now")),
            "technology": sys_trim(&path.join("technology")),
            "manufacturer": sys_trim(&path.join("manufacturer")),
            "model_name": sys_trim(&path.join("model_name")),
            "cycle_count": sys_trim(&path.join("cycle_count")),
        }));
    }
    json!({
        "kind": OBSERVATION,
        "source": "/sys/class/power_supply",
        "batteries": batteries,
        "note": "Capacity and status are firmware reports. A missing or odd value is not by itself a confirmed hardware failure.",
    })
}

pub fn network_info() -> Value {
    let mut interfaces = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !valid_iface(&name) {
                continue;
            }
            let path = entry.path();
            interfaces.push(json!({
                "name": name,
                "operstate": sys_trim(&path.join("operstate")),
                "mtu": sys_trim(&path.join("mtu")),
                "mac": sys_trim(&path.join("address")),
            }));
            if interfaces.len() == 32 {
                break;
            }
        }
    }
    let route = read_text("/proc/net/route").and_then(|text| parse_ipv4_route(&text));
    json!({
        "kind": OBSERVATION,
        "interfaces": interfaces,
        "default_route_ipv4": route,
        "addresses": ip_addresses(),
        "note": "Includes MAC addresses and local IPs. DNS search domains are not collected.",
    })
}

pub fn hardware_devices() -> Value {
    let mut devices = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/bus/pci/devices") {
        for entry in entries.flatten() {
            let path = entry.path();
            let class = sys_trim(&path.join("class")).unwrap_or_default();
            devices.push(json!({
                "slot": entry.file_name().to_string_lossy(),
                "class": class,
                "class_name": pci_class_name(&class),
                "vendor": sys_trim(&path.join("vendor")),
                "device": sys_trim(&path.join("device")),
            }));
            if devices.len() == 64 {
                break;
            }
        }
    }
    json!({
        "kind": OBSERVATION,
        "source": "/sys/bus/pci/devices",
        "pci": devices,
        "dmi": dmi_info(),
        "note": "Serial numbers and UUIDs are not collected.",
    })
}

pub fn process_info(pid: Option<u32>) -> Result<Value> {
    let Some(pid) = pid else {
        return Ok(json!({
            "kind": OBSERVATION,
            "process_count": count_pids(),
            "self_pid": std::process::id(),
            "loadavg": read_text("/proc/loadavg").unwrap_or_default().trim(),
            "note": "Pass a pid to inspect one process. environ and command lines are not read.",
        }));
    };
    if pid == 0 || pid > 4_194_304 {
        return Err(Error::InvalidInput("pid is out of range".into()));
    }
    let status = std::fs::read_to_string(format!("/proc/{pid}/status"))
        .map_err(|_| Error::NotFound(format!("pid {pid}")))?;
    let wanted = ["Name", "State", "Pid", "PPid", "Uid", "VmRSS", "Threads"];
    let mut picked = Map::new();
    for line in status.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if wanted.contains(&key.trim()) {
            picked.insert(
                key.trim().to_string(),
                Value::String(value.trim().to_string()),
            );
        }
    }
    Ok(json!({
        "kind": OBSERVATION,
        "process": Value::Object(picked),
    }))
}

pub fn package_info(name: Option<&str>) -> Result<Value> {
    let managers = detect_managers();
    let Some(name) = name else {
        return Ok(json!({
            "kind": OBSERVATION,
            "managers": managers,
            "note": "Pass a package name to query one installed package.",
        }));
    };
    validate_package_name(name)?;
    if let Some(bin) = find_program(&["/usr/bin/dpkg-query", "dpkg-query"]) {
        return dpkg_query(&bin, name);
    }
    if let Some(bin) = find_program(&["/usr/bin/rpm", "rpm"]) {
        return rpm_query(&bin, name);
    }
    Ok(json!({
        "kind": OBSERVATION,
        "managers": managers,
        "installed": Value::Null,
        "reason": "no supported package query tool",
    }))
}

pub fn service_status(scope: &str, name: Option<&str>) -> Result<Value> {
    if scope != "user" && scope != "system" {
        return Err(Error::InvalidInput(
            "scope must be 'user' or 'system'".into(),
        ));
    }
    let Some(bin) = find_program(&["/usr/bin/systemctl", "systemctl"]) else {
        return Ok(
            json!({"kind": OBSERVATION, "available": false, "reason": "systemctl not found"}),
        );
    };
    let Some(name) = name else {
        return Ok(json!({
            "kind": OBSERVATION,
            "available": true,
            "scope": scope,
            "note": "Pass a unit name to read one unit. This does not list every unit.",
        }));
    };
    validate_unit_name(name)?;
    let mut args = vec![
        "show",
        "-p",
        "Id",
        "-p",
        "LoadState",
        "-p",
        "ActiveState",
        "-p",
        "SubState",
        "-p",
        "UnitFileState",
    ];
    if scope == "user" {
        args.insert(1, "--user");
    }
    args.push("--");
    args.push(name);
    let text = run_argv(&bin, &args, Duration::from_secs(5))?;
    Ok(json!({
        "kind": OBSERVATION,
        "scope": scope,
        "properties": systemctl_map(&text),
    }))
}

fn dpkg_query(bin: &Path, name: &str) -> Result<Value> {
    match run_argv(
        bin,
        &["-W", "-f=${Package}\t${Version}\n", name],
        Duration::from_secs(5),
    ) {
        Ok(text) => Ok(
            json!({"kind": OBSERVATION, "manager": "dpkg", "installed": true, "record": text.trim()}),
        ),
        Err(Error::Other(msg)) if msg.contains("exited 1") => {
            Ok(json!({"kind": OBSERVATION, "manager": "dpkg", "installed": false, "name": name}))
        }
        Err(err) => Err(err),
    }
}

fn rpm_query(bin: &Path, name: &str) -> Result<Value> {
    match run_argv(
        bin,
        &[
            "-q",
            "--queryformat",
            "%{NAME} %{VERSION}-%{RELEASE}\n",
            name,
        ],
        Duration::from_secs(5),
    ) {
        Ok(text) => Ok(
            json!({"kind": OBSERVATION, "manager": "rpm", "installed": true, "record": text.trim()}),
        ),
        Err(Error::Other(msg)) if msg.contains("exited 1") => {
            Ok(json!({"kind": OBSERVATION, "manager": "rpm", "installed": false, "name": name}))
        }
        Err(err) => Err(err),
    }
}

fn detect_managers() -> Vec<&'static str> {
    let mut found = Vec::new();
    for (label, candidates) in [
        ("dpkg", &["/usr/bin/dpkg-query"][..]),
        ("rpm", &["/usr/bin/rpm"][..]),
        ("pacman", &["/usr/bin/pacman"][..]),
        ("apk", &["/sbin/apk"][..]),
        ("flatpak", &["/usr/bin/flatpak"][..]),
    ] {
        if find_program(candidates).is_some() {
            found.push(label);
        }
    }
    found
}

fn ip_addresses() -> Value {
    let Some(bin) = find_program(&["/usr/sbin/ip", "/sbin/ip", "/usr/bin/ip", "ip"]) else {
        return json!({"available": false, "reason": "ip command not found"});
    };
    let text = match run_argv(&bin, &["-json", "addr", "show"], Duration::from_secs(3)) {
        Ok(text) => text,
        Err(err) => return json!({"available": false, "reason": err.to_string()}),
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return json!({"available": false, "reason": "ip did not return JSON"});
    };
    let Some(list) = value.as_array() else {
        return json!({"available": false, "reason": "unexpected ip JSON"});
    };
    let mut addresses = Vec::new();
    for iface in list {
        let name = iface.get("ifname").and_then(Value::as_str).unwrap_or("");
        let Some(infos) = iface.get("addr_info").and_then(Value::as_array) else {
            continue;
        };
        for info in infos {
            addresses.push(json!({
                "ifname": name,
                "family": info.get("family"),
                "local": info.get("local"),
                "prefixlen": info.get("prefixlen"),
            }));
            if addresses.len() == 64 {
                return json!({"available": true, "items": addresses, "truncated": true});
            }
        }
    }
    json!({"available": true, "items": addresses})
}

fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    let stat = nix::sys::statvfs::statvfs(path).ok()?;
    let frag = stat.fragment_size();
    let total = stat.blocks().saturating_mul(frag);
    let avail = stat.blocks_available().saturating_mul(frag);
    Some((total, avail))
}

fn dmi_info() -> Value {
    let base = Path::new("/sys/class/dmi/id");
    if !base.is_dir() {
        return json!({"available": false});
    }
    json!({
        "available": true,
        "sys_vendor": sys_trim(&base.join("sys_vendor")),
        "product_name": sys_trim(&base.join("product_name")),
        "board_vendor": sys_trim(&base.join("board_vendor")),
        "bios_version": sys_trim(&base.join("bios_version")),
    })
}

fn systemctl_map(text: &str) -> Map<String, Value> {
    let mut map = Map::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        map.insert(key.to_string(), Value::String(value.to_string()));
    }
    map
}

fn driver_name(device: &Path) -> Option<String> {
    let link = std::fs::read_link(device.join("driver")).ok()?;
    link.file_name().map(|n| n.to_string_lossy().into_owned())
}

fn is_drm_card(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("card") else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn pci_class_name(class: &str) -> &'static str {
    let hex = class.trim_start_matches("0x");
    let Ok(value) = u32::from_str_radix(hex, 16) else {
        return "unknown";
    };
    match value >> 16 {
        0x01 => "mass storage",
        0x02 => "network",
        0x03 => "display",
        0x04 => "multimedia",
        0x06 => "bridge",
        0x0c => "serial bus",
        _ => "other",
    }
}

fn count_pids() -> usize {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()))
        })
        .count()
}

fn valid_iface(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

pub fn validate_package_name(name: &str) -> Result<()> {
    let bytes = name.as_bytes();
    let Some((first, rest)) = bytes.split_first() else {
        return Err(Error::InvalidInput("package name is empty".into()));
    };
    if first.is_ascii_alphanumeric()
        && rest.len() <= 127
        && rest.iter().copied().all(package_name_byte)
    {
        return Ok(());
    }
    Err(Error::InvalidInput("package name is not valid".into()))
}

fn package_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'_' | b'-' | b':')
}

pub fn validate_unit_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 256 {
        return Err(Error::InvalidInput("unit name is not valid".into()));
    }
    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '-' | ':' | '\\'));
    if !ok {
        return Err(Error::InvalidInput("unit name is not valid".into()));
    }
    Ok(())
}

fn read_text(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn sys_trim(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(clip_owned(trimmed, 200))
}

fn clip_owned(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_shell_metacharacters_in_names() {
        assert!(validate_package_name("bash").is_ok());
        assert!(validate_package_name("libc6").is_ok());
        assert!(validate_package_name("foo;rm -rf").is_err());
        assert!(validate_package_name("../etc").is_err());
        assert!(validate_package_name("$(id)").is_err());
        assert!(validate_unit_name("NetworkManager.service").is_ok());
        assert!(validate_unit_name("foo.service;reboot").is_err());
        assert!(validate_unit_name("unit name.service").is_err());
    }

    #[test]
    fn live_memory_is_real() {
        let value = memory_info();
        let total = value["memory"]["mem_total_kib"].as_u64().unwrap_or(0);
        assert!(total > 0, "expected MemTotal from /proc/meminfo");
    }
}
