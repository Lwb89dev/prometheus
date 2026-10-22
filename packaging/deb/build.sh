#!/bin/sh
# Build a .deb from a release cargo build. Does not install it and does not
# enable the systemd user unit.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
arch=$(dpkg --print-architecture)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cargo build --release --locked -p lda-daemon -p lda-cli -p lda-gtk
pkg="$stage/lda_${version}_${arch}"
mkdir -p "$pkg/usr/bin" "$pkg/DEBIAN" \
    "$pkg/usr/share/applications" \
    "$pkg/usr/share/icons/hicolor/scalable/apps" \
    "$pkg/usr/share/dbus-1/services" \
    "$pkg/usr/lib/systemd/user"
install -m 0755 target/release/lda-daemon target/release/lda-cli target/release/lda-gtk \
    "$pkg/usr/bin/"
install -m 0644 packaging/desktop/org.linuxdesktopagent.Gtk.desktop \
    "$pkg/usr/share/applications/"
install -m 0644 packaging/icons/hicolor/scalable/apps/org.linuxdesktopagent.Gtk.svg \
    "$pkg/usr/share/icons/hicolor/scalable/apps/"
install -m 0644 packaging/dbus/org.linuxdesktopagent.Daemon.service \
    "$pkg/usr/share/dbus-1/services/"
sed 's|%h/.local/bin/lda-daemon|/usr/bin/lda-daemon|' \
    packaging/systemd/user/lda-daemon.service \
    > "$pkg/usr/lib/systemd/user/lda-daemon.service"
chmod 0644 "$pkg/usr/lib/systemd/user/lda-daemon.service"
cat > "$pkg/DEBIAN/control" <<EOF
Package: lda
Version: ${version}
Section: utils
Priority: optional
Architecture: ${arch}
Depends: libgtk-4-1, libsqlite3-0
Maintainer: Linux Desktop Agent <linux-desktop-agent@localhost>
Description: Local personal assistant for the Linux desktop
 Unprivileged daemon, GTK client, and command line.
 The systemd user unit is installed and not enabled.
EOF
find "$pkg" -type d -exec chmod 0755 {} +
mkdir -p "$root/target"
dpkg-deb --root-owner-group --build "$pkg" "$root/target/lda_${version}_${arch}.deb"
echo "wrote $root/target/lda_${version}_${arch}.deb"
