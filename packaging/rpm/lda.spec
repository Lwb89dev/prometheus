# RPM spec for Linux Desktop Agent.
# This file is the recipe. It was not executed unless DEVELOPMENT.md says
# rpmbuild ran on a named host. The package does not enable a systemd unit.
Name:           lda
Version:        0.2.0
Release:        1%{?dist}
Summary:        Local personal assistant for the Linux desktop
License:        GPL-3.0-or-later
URL:            https://github.com/Lwb89dev/prometheus
BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gtk4-devel
BuildRequires:  sqlite-devel
BuildRequires:  pkgconfig
Requires:       gtk4
Requires:       sqlite-libs

%description
Unprivileged daemon, GTK 4 client, and command line for a local personal
assistant. The systemd user unit is packaged and not enabled.

%prep
# Build from the repository root that contains this spec's sources.

%build
cargo build --release --locked -p lda-daemon -p lda-cli -p lda-gtk

%install
install -D -m 0755 target/release/lda-daemon %{buildroot}/usr/bin/lda-daemon
install -D -m 0755 target/release/lda-cli %{buildroot}/usr/bin/lda-cli
install -D -m 0755 target/release/lda-gtk %{buildroot}/usr/bin/lda-gtk
install -D -m 0644 packaging/desktop/org.linuxdesktopagent.Gtk.desktop \
    %{buildroot}/usr/share/applications/org.linuxdesktopagent.Gtk.desktop
install -D -m 0644 packaging/icons/hicolor/scalable/apps/org.linuxdesktopagent.Gtk.svg \
    %{buildroot}/usr/share/icons/hicolor/scalable/apps/org.linuxdesktopagent.Gtk.svg
install -D -m 0644 packaging/dbus/org.linuxdesktopagent.Daemon.service \
    %{buildroot}/usr/share/dbus-1/services/org.linuxdesktopagent.Daemon.service
mkdir -p %{buildroot}/usr/lib/systemd/user
sed 's|%h/.local/bin/lda-daemon|/usr/bin/lda-daemon|' \
    packaging/systemd/user/lda-daemon.service \
    > %{buildroot}/usr/lib/systemd/user/lda-daemon.service
chmod 0644 %{buildroot}/usr/lib/systemd/user/lda-daemon.service

%files
/usr/bin/lda-daemon
/usr/bin/lda-cli
/usr/bin/lda-gtk
/usr/share/applications/org.linuxdesktopagent.Gtk.desktop
/usr/share/icons/hicolor/scalable/apps/org.linuxdesktopagent.Gtk.svg
/usr/share/dbus-1/services/org.linuxdesktopagent.Daemon.service
/usr/lib/systemd/user/lda-daemon.service
