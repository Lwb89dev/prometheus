use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OsRelease {
    pub name: String,
    pub id: String,
    pub version_id: String,
    pub pretty_name: String,
    pub id_like: String,
}

impl Default for OsRelease {
    fn default() -> Self {
        Self {
            name: "unknown".into(),
            id: "unknown".into(),
            version_id: String::new(),
            pretty_name: "unknown".into(),
            id_like: String::new(),
        }
    }
}

pub fn parse_os_release(text: &str) -> OsRelease {
    let map = parse_assign_file(text);
    let mut info = OsRelease::default();
    if let Some(name) = map.get("NAME") {
        info.name.clone_from(name);
        info.pretty_name.clone_from(name);
    }
    if let Some(id) = map.get("ID") {
        info.id.clone_from(id);
    }
    if let Some(version) = map.get("VERSION_ID") {
        info.version_id.clone_from(version);
    }
    if let Some(pretty) = map.get("PRETTY_NAME") {
        info.pretty_name.clone_from(pretty);
    }
    if let Some(like) = map.get("ID_LIKE") {
        info.id_like.clone_from(like);
    }
    info
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CpuInfo {
    pub model: String,
    pub vendor: String,
    pub logical_cpus: u32,
    pub cores_per_socket: Option<u32>,
    pub has_vmx: bool,
    pub has_svm: bool,
}

pub fn parse_cpuinfo(text: &str) -> CpuInfo {
    let mut model = String::new();
    let mut vendor = String::new();
    let mut logical = 0u32;
    let mut cores = None;
    let mut has_vmx = false;
    let mut has_svm = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "processor" => logical = logical.saturating_add(1),
            "model name" if model.is_empty() => model = value.to_string(),
            "vendor_id" if vendor.is_empty() => vendor = value.to_string(),
            "cpu cores" if cores.is_none() => cores = value.parse().ok(),
            "flags" | "Features" => {
                for flag in value.split_whitespace() {
                    if flag == "vmx" {
                        has_vmx = true;
                    }
                    if flag == "svm" {
                        has_svm = true;
                    }
                }
            }
            _ => {}
        }
    }
    CpuInfo {
        model,
        vendor,
        logical_cpus: logical,
        cores_per_socket: cores,
        has_vmx,
        has_svm,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryInfo {
    pub mem_total_kib: u64,
    pub mem_available_kib: Option<u64>,
    pub mem_free_kib: Option<u64>,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
    pub source: &'static str,
}

pub fn parse_meminfo(text: &str) -> MemoryInfo {
    let map = kib_map(text);
    MemoryInfo {
        mem_total_kib: map.get("MemTotal").copied().unwrap_or(0),
        mem_available_kib: map.get("MemAvailable").copied(),
        mem_free_kib: map.get("MemFree").copied(),
        swap_total_kib: map.get("SwapTotal").copied().unwrap_or(0),
        swap_free_kib: map.get("SwapFree").copied().unwrap_or(0),
        source: "/proc/meminfo",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MountEntry {
    pub source: String,
    pub target: String,
    pub fstype: String,
}

pub fn parse_mounts(text: &str) -> Vec<MountEntry> {
    let mut mounts = Vec::new();
    for line in text.lines() {
        let mut parts = split_mount_fields(line);
        if parts.len() < 3 {
            continue;
        }
        let fstype = parts.remove(2);
        if !interesting_fs(&fstype) {
            continue;
        }
        let source = parts.remove(0);
        let target = parts.remove(0);
        mounts.push(MountEntry {
            source,
            target,
            fstype,
        });
        if mounts.len() == 32 {
            break;
        }
    }
    mounts
}

pub fn parse_uptime(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

pub fn parse_btime(text: &str) -> Option<i64> {
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("btime ") else {
            continue;
        };
        return rest.trim().parse().ok();
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DefaultRoute {
    pub interface: String,
    pub gateway: String,
}

pub fn parse_ipv4_route(text: &str) -> Option<DefaultRoute> {
    for line in text.lines().skip(1) {
        let mut fields = line.split_whitespace();
        let iface = fields.next()?;
        let destination = fields.next()?;
        let gateway = fields.next()?;
        if destination == "00000000" {
            return Some(DefaultRoute {
                interface: iface.to_string(),
                gateway: hex_le_ipv4(gateway).unwrap_or_else(|| gateway.to_string()),
            });
        }
    }
    None
}

pub fn hex_le_ipv4(hex: &str) -> Option<String> {
    if hex.len() != 8 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let bytes = hex.as_bytes();
    let mut octets = [0u8; 4];
    for (i, octet) in octets.iter_mut().enumerate() {
        let start = (3 - i) * 2;
        *octet =
            u8::from_str_radix(std::str::from_utf8(&bytes[start..start + 2]).ok()?, 16).ok()?;
    }
    Some(format!(
        "{}.{}.{}.{}",
        octets[0], octets[1], octets[2], octets[3]
    ))
}

fn parse_assign_file(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        map.insert(key.trim().to_string(), unquote(value.trim()));
    }
    map
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    let inner = if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    };
    inner.replace("\\\"", "\"").replace("\\\\", "\\")
}

fn kib_map(text: &str) -> BTreeMap<String, u64> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let number = rest.split_whitespace().next().unwrap_or("");
        if let Ok(value) = number.parse::<u64>() {
            map.insert(key.trim().to_string(), value);
        }
    }
    map
}

fn interesting_fs(fstype: &str) -> bool {
    matches!(
        fstype,
        "ext4"
            | "btrfs"
            | "xfs"
            | "f2fs"
            | "vfat"
            | "exfat"
            | "ntfs"
            | "ntfs3"
            | "zfs"
            | "overlay"
            | "fuseblk"
            | "bcachefs"
            | "tmpfs"
    )
}

fn split_mount_fields(line: &str) -> Vec<String> {
    line.split_whitespace().map(unescape_mount).collect()
}

fn unescape_mount(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            let oct = &value[i + 1..i + 4];
            if let Ok(decoded) = u8::from_str_radix(oct, 8) {
                out.push(decoded as char);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_pop() {
        let text = "NAME=\"Pop!_OS\"\nID=pop\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Pop!_OS 24.04 LTS\"\nID_LIKE=\"ubuntu debian\"\n";
        let info = parse_os_release(text);
        assert_eq!(info.id, "pop");
        assert_eq!(info.version_id, "24.04");
        assert_eq!(info.pretty_name, "Pop!_OS 24.04 LTS");
        assert_eq!(info.id_like, "ubuntu debian");
    }

    #[test]
    fn meminfo_and_cpu() {
        let mem = parse_meminfo(
            "MemTotal: 1000 kB\nMemAvailable: 400 kB\nSwapTotal: 10 kB\nSwapFree: 10 kB\n",
        );
        assert_eq!(mem.mem_total_kib, 1000);
        assert_eq!(mem.mem_available_kib, Some(400));
        let cpu = parse_cpuinfo("processor : 0\nmodel name : Test CPU\nvendor_id : GenuineIntel\ncpu cores : 4\nflags : fpu vmx aes\nprocessor : 1\n");
        assert_eq!(cpu.logical_cpus, 2);
        assert_eq!(cpu.model, "Test CPU");
        assert!(cpu.has_vmx);
        assert!(!cpu.has_svm);
    }

    #[test]
    fn mounts_unescape_and_filter() {
        let text = "proc /proc proc rw 0 0\n/dev/nvme0n1p3 / ext4 rw 0 0\n/dev/sda1 /mnt/My\\040Disk btrfs rw 0 0\n";
        let mounts = parse_mounts(text);
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[1].target, "/mnt/My Disk");
    }

    #[test]
    fn default_route_hex() {
        assert_eq!(hex_le_ipv4("0101A8C0").as_deref(), Some("192.168.1.1"));
        let text = "Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT\nwlan0 00000000 0101A8C0 0003 0 0 100 00000000 0 0 0\n";
        let route = parse_ipv4_route(text).unwrap();
        assert_eq!(route.interface, "wlan0");
        assert_eq!(route.gateway, "192.168.1.1");
    }
}
