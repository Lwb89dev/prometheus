use serde::Serialize;

/// Inputs for desktop detection. Tests pass fixtures; the live path reads the environment.
#[derive(Debug, Clone, Default)]
pub struct DesktopProbe {
    pub xdg_current_desktop: Option<String>,
    pub xdg_session_type: Option<String>,
    pub xdg_session_desktop: Option<String>,
    pub wayland_display: Option<String>,
    pub display: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesktopInfo {
    pub desktops: Vec<String>,
    pub primary: String,
    pub session_type: String,
    pub session_desktop: Option<String>,
    pub compositor_hint: Option<String>,
}

impl DesktopProbe {
    pub fn from_env() -> Self {
        Self {
            xdg_current_desktop: env_string("XDG_CURRENT_DESKTOP"),
            xdg_session_type: env_string("XDG_SESSION_TYPE"),
            xdg_session_desktop: env_string("XDG_SESSION_DESKTOP"),
            wayland_display: env_string("WAYLAND_DISPLAY"),
            display: env_string("DISPLAY"),
        }
    }
}

pub fn classify_desktop(probe: &DesktopProbe) -> DesktopInfo {
    let desktops = probe
        .xdg_current_desktop
        .as_deref()
        .map(split_desktop_list)
        .unwrap_or_default();
    let primary = desktops
        .first()
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let session_type = session_type(probe);
    let compositor_hint = compositor_hint(&primary, &session_type);
    DesktopInfo {
        desktops,
        primary,
        session_type,
        session_desktop: probe.xdg_session_desktop.clone(),
        compositor_hint,
    }
}

fn split_desktop_list(value: &str) -> Vec<String> {
    value
        .split(':')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn session_type(probe: &DesktopProbe) -> String {
    if let Some(value) = probe.xdg_session_type.as_deref() {
        let lower = value.trim().to_ascii_lowercase();
        if lower == "wayland" || lower == "x11" || lower == "tty" {
            return lower;
        }
    }
    if probe
        .wayland_display
        .as_ref()
        .is_some_and(|v| !v.is_empty())
    {
        return "wayland".to_string();
    }
    if probe.display.as_ref().is_some_and(|v| !v.is_empty()) {
        return "x11".to_string();
    }
    "unknown".to_string()
}

fn compositor_hint(primary: &str, session_type: &str) -> Option<String> {
    let name = primary.to_ascii_lowercase();
    let hint = match name.as_str() {
        "cosmic" => "cosmic-comp",
        "gnome" if session_type == "wayland" => "mutter",
        "kde" | "kde-plasma" if session_type == "wayland" => "kwin",
        "xfce" => "xfwm4",
        _ => return None,
    };
    Some(hint.to_string())
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosmic_wayland_is_not_reported_as_gnome() {
        let info = classify_desktop(&DesktopProbe {
            xdg_current_desktop: Some("COSMIC".into()),
            xdg_session_type: Some("wayland".into()),
            xdg_session_desktop: Some("COSMIC".into()),
            wayland_display: Some("wayland-1".into()),
            display: Some(":1".into()),
        });
        assert_eq!(info.primary, "COSMIC");
        assert_eq!(info.session_type, "wayland");
        assert_eq!(info.compositor_hint.as_deref(), Some("cosmic-comp"));
        assert!(!info.desktops.iter().any(|d| d == "GNOME"));
    }

    #[test]
    fn x11_is_not_assumed_from_wayland() {
        let info = classify_desktop(&DesktopProbe {
            xdg_current_desktop: Some("GNOME".into()),
            xdg_session_type: Some("wayland".into()),
            xdg_session_desktop: None,
            wayland_display: Some("wayland-0".into()),
            display: None,
        });
        assert_eq!(info.session_type, "wayland");
    }

    #[test]
    fn unknown_when_nothing_is_set() {
        let info = classify_desktop(&DesktopProbe::default());
        assert_eq!(info.session_type, "unknown");
        assert_eq!(info.primary, "unknown");
    }
}
