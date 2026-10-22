//! XDG desktop portal calls. Each one fails closed when the interface is absent.

use std::collections::HashMap;

use lda_common::{Error, Result};
use lda_system::DesktopInfo;
use zbus::zvariant::Value;
use zbus::{proxy, Connection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCaps {
    pub desktop: String,
    pub session_type: String,
    pub notification: bool,
    pub open_uri: bool,
    pub global_shortcuts: bool,
    pub layer_shell: bool,
    pub tested_here: bool,
}

pub fn caps_from_introspection(info: &DesktopInfo, xml: &str, layer_shell: bool) -> HostCaps {
    HostCaps {
        desktop: info.primary.clone(),
        session_type: info.session_type.clone(),
        notification: xml.contains("org.freedesktop.portal.Notification"),
        open_uri: xml.contains("org.freedesktop.portal.OpenURI"),
        global_shortcuts: xml.contains("org.freedesktop.portal.GlobalShortcuts"),
        layer_shell,
        tested_here: matches!(
            info.primary.as_str(),
            "COSMIC" | "cosmic" | "GNOME" | "gnome"
        ),
    }
}

pub fn session_note(caps: &HostCaps) -> String {
    let mut parts = Vec::new();
    parts.push(format!("{} on {}", caps.desktop, caps.session_type));
    if caps.global_shortcuts {
        parts.push("GlobalShortcuts portal is present".to_string());
    } else {
        parts.push("no global shortcut is registered; Super+Space is left untouched".to_string());
    }
    if caps.layer_shell {
        parts.push("layer-shell library is installed".to_string());
    } else {
        parts.push("the assistant mark stays in a normal GTK window".to_string());
    }
    if !caps.notification {
        parts.push("notification portal was not found".to_string());
    }
    if !caps.tested_here {
        parts.push(format!(
            "{} was not exercised on this machine",
            caps.desktop
        ));
    }
    parts.join(". ")
}

pub async fn probe(info: &DesktopInfo) -> HostCaps {
    let xml = introspection().await.unwrap_or_default();
    caps_from_introspection(info, &xml, layer_shell_present())
}

pub async fn notify(title: &str, body: &str) -> Result<()> {
    let connection = session().await?;
    let proxy = NotificationProxy::new(&connection)
        .await
        .map_err(|err| Error::Other(err.to_string()))?;
    let mut notification: HashMap<&str, Value<'static>> = HashMap::new();
    notification.insert("title", Value::from(truncate(title, 80)));
    notification.insert("body", Value::from(truncate(body, 180)));
    notification.insert("priority", Value::from("normal".to_string()));
    proxy
        .add_notification("linux-desktop-agent", notification)
        .await
        .map_err(|err| Error::Other(err.to_string()))
}

pub async fn open_uri(uri: &str) -> Result<()> {
    if !(uri.starts_with("https://") || uri.starts_with("http://") || uri.starts_with("file://")) {
        return Err(Error::InvalidInput(
            "only http, https, and file URIs can be opened".into(),
        ));
    }
    let connection = session().await?;
    let proxy = OpenUriProxy::new(&connection)
        .await
        .map_err(|err| Error::Other(err.to_string()))?;
    let options: HashMap<&str, Value> = HashMap::new();
    proxy
        .open_uri("", uri, options)
        .await
        .map_err(|err| Error::Other(err.to_string()))?;
    Ok(())
}

fn layer_shell_present() -> bool {
    std::process::Command::new("pkg-config")
        .args(["--exists", "gtk4-layer-shell-0"])
        .status()
        .is_ok_and(|status| status.success())
}

async fn introspection() -> Result<String> {
    let connection = session().await?;
    let reply = connection
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.DBus.Introspectable"),
            "Introspect",
            &(),
        )
        .await
        .map_err(|err| Error::Other(err.to_string()))?;
    reply
        .body()
        .deserialize::<String>()
        .map_err(|err| Error::Other(err.to_string()))
}

async fn session() -> Result<Connection> {
    Connection::session()
        .await
        .map_err(|err| Error::Other(format!("session bus: {err}")))
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[proxy(
    gen_blocking = false,
    interface = "org.freedesktop.portal.Notification",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait Notification {
    fn add_notification(
        &self,
        id: &str,
        notification: HashMap<&str, Value<'_>>,
    ) -> zbus::Result<()>;
}

#[proxy(
    gen_blocking = false,
    interface = "org.freedesktop.portal.OpenURI",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait OpenUri {
    fn open_uri(
        &self,
        parent_window: &str,
        uri: &str,
        options: HashMap<&str, Value<'_>>,
    ) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use lda_system::DesktopProbe;

    #[test]
    fn cosmic_fixture_has_notification_and_no_shortcut() {
        let info = classify();
        let xml = r#"
            <interface name="org.freedesktop.portal.Notification"></interface>
            <interface name="org.freedesktop.portal.OpenURI"></interface>
        "#;
        let caps = caps_from_introspection(&info, xml, false);
        assert!(caps.notification);
        assert!(caps.open_uri);
        assert!(!caps.global_shortcuts);
        assert!(!caps.layer_shell);
        let note = session_note(&caps);
        assert!(note.contains("Super+Space"));
        assert!(note.contains("normal GTK window"));
    }

    #[test]
    fn kde_is_not_marked_tested() {
        let info = lda_system::classify_desktop(&DesktopProbe {
            xdg_current_desktop: Some("KDE".into()),
            xdg_session_type: Some("wayland".into()),
            ..DesktopProbe::default()
        });
        let caps = caps_from_introspection(&info, "", false);
        assert!(!caps.tested_here);
        assert!(session_note(&caps).contains("not exercised"));
    }

    #[tokio::test]
    #[ignore = "shows one real notification on the session bus"]
    async fn live_notification_when_portal_exists() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            return;
        }
        notify("Linux Desktop Agent", "Notification portal check")
            .await
            .unwrap();
    }

    fn classify() -> DesktopInfo {
        lda_system::classify_desktop(&DesktopProbe {
            xdg_current_desktop: Some("COSMIC".into()),
            xdg_session_type: Some("wayland".into()),
            ..DesktopProbe::default()
        })
    }
}
