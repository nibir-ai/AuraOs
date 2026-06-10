#!/usr/bin/env bash
# build-iso.sh — AuraOS ISO Generation Script
#
# Bootstraps an Ubuntu 26.04 LTS (Resolute Raccoon) chroot, injects custom configurations,
# compiled AuraOS Debian packages, custom installer, and outputs a bootable hybrid ISO image.
#
# Copyright (C) 2025 AuraOS Contributors
# SPDX-License-Identifier: GPL-3.0-or-later

set -euo pipefail

# Configuration
CODENAME="resolute"
ARCH="amd64"
ROOT_DIR="chroot"
IMAGE_DIR="image"
ISO_NAME="auraos-26.04-desktop-${ARCH}.iso"
DEB_REPO_DIR="../build"

echo "=== AuraOS ISO Build System ==="

# 1. Install required host dependencies
echo "Installing build dependencies..."
apt-get update && apt-get install -y \
    debootstrap \
    squashfs-tools \
    xorriso \
    grub-pc-bin \
    grub-efi-amd64-bin \
    mtools \
    dosfstools \
    syslinux \
    isolinux

# 2. Bootstrap base chroot filesystem
if [ ! -d "${ROOT_DIR}" ]; then
    echo "Bootstrapping minimal Ubuntu ${CODENAME} chroot..."
    debootstrap --arch="${ARCH}" "${CODENAME}" "${ROOT_DIR}" http://archive.ubuntu.com/ubuntu/
else
    echo "Using existing chroot directory."
fi

# 3. Mount kernel virtual filesystems inside chroot
mount_chroot() {
    echo "Mounting chroot virtual filesystems..."
    mount -t proc proc "${ROOT_DIR}/proc"
    mount -t sysfs sys "${ROOT_DIR}/sys"
    mount -o bind /dev "${ROOT_DIR}/dev"
    mount -t devpts pts "${ROOT_DIR}/dev/pts"
}

unmount_chroot() {
    echo "Unmounting chroot virtual filesystems..."
    umount -lf "${ROOT_DIR}/proc" || true
    umount -lf "${ROOT_DIR}/sys" || true
    umount -lf "${ROOT_DIR}/dev/pts" || true
    umount -lf "${ROOT_DIR}/dev" || true
}

trap unmount_chroot EXIT
mount_chroot

# 4. Configure chroot packages and repositories (Ubuntu 26.04 Resolute Raccoon)
echo "Configuring apt repositories inside chroot..."
cat <<EOF > "${ROOT_DIR}/etc/apt/sources.list"
deb http://archive.ubuntu.com/ubuntu/ resolute main restricted universe multiverse
deb http://archive.ubuntu.com/ubuntu/ resolute-updates main restricted universe multiverse
deb http://archive.ubuntu.com/ubuntu/ resolute-security main restricted universe multiverse
EOF

# Copy resolv.conf for internet access inside chroot
cp /etc/resolv.conf "${ROOT_DIR}/etc/resolv.conf"

# 5. Run configuration script inside chroot
echo "Provisioning packages inside chroot..."
cat <<'EOF' > "${ROOT_DIR}/tmp/provision.sh"
#!/bin/bash
set -ex
export DEBIAN_FRONTEND=noninteractive

# Update and install base desktop components
apt-get update
apt-get install -y --no-install-recommends \
    ubuntu-desktop-minimal \
    gnome-shell \
    gdm3 \
    network-manager \
    systemd-resolved \
    ca-certificates \
    curl \
    git \
    sudo \
    policykit-1 \
    libsecret-1-0 \
    gnome-keyring \
    google-drive-ocamlfuse \
    evolution-data-server \
    fuse3 \
    libadwaita-1-0 \
    fastfetch

# Enable systemd services
systemctl enable gdm3
systemctl enable NetworkManager
systemctl enable systemd-resolved

# Configure GDM automatic login for live ISO
mkdir -p /etc/gdm3
cat <<GDM > /etc/gdm3/custom.conf
[daemon]
AutomaticLoginEnable=true
AutomaticLogin=live
GDM

# Clean cache to reduce SquashFS size
apt-get clean
EOF

chmod +x "${ROOT_DIR}/tmp/provision.sh"
chroot "${ROOT_DIR}" /tmp/provision.sh
rm "${ROOT_DIR}/tmp/provision.sh"

# 6. Copy AuraOS Custom Debian Packages into chroot and install
echo "Injecting AuraOS packages..."
mkdir -p "${ROOT_DIR}/tmp/auraos-pkgs"
cp "${DEB_REPO_DIR}"/*.deb "${ROOT_DIR}/tmp/auraos-pkgs/" || echo "Warning: No .deb files found in ../build/. Skipping local package insertion."

cat <<'EOF' > "${ROOT_DIR}/tmp/install-pkgs.sh"
#!/bin/bash
set -ex
if [ -d /tmp/auraos-pkgs ] && [ "$(ls -A /tmp/auraos-pkgs)" ]; then
    apt-get install -y /tmp/auraos-pkgs/*.deb
fi
rm -rf /tmp/auraos-pkgs
EOF
chmod +x "${ROOT_DIR}/tmp/install-pkgs.sh"
chroot "${ROOT_DIR}" /tmp/install-pkgs.sh
rm "${ROOT_DIR}/tmp/install-pkgs.sh"

# 7. Configure GNOME Shell extension and theme defaults (Dark Mode & Blue Accent & Wallpaper)
echo "Setting GNOME default overrides..."

# Copy custom wallpaper from host installer directory to target chroot
mkdir -p "${ROOT_DIR}/usr/share/backgrounds/auraos"
cp ../installer/branding/auraos/wallpaper.jpg "${ROOT_DIR}/usr/share/backgrounds/auraos/wallpaper.jpg"

mkdir -p "${ROOT_DIR}/usr/share/glib-2.0/schemas"
cat <<EOF > "${ROOT_DIR}/usr/share/glib-2.0/schemas/99-auraos-defaults.gschema.override"
[org.gnome.shell]
enabled-extensions=['gemini-assistant@auraos.org', 'aura-account-switcher']

[org.gnome.desktop.interface]
enable-hot-corners=false
color-scheme='prefer-dark'
accent-color='blue'

[org.gnome.desktop.background]
picture-uri='file:///usr/share/backgrounds/auraos/wallpaper.jpg'
picture-uri-dark='file:///usr/share/backgrounds/auraos/wallpaper.jpg'
picture-options='zoom'

[org.gnome.desktop.screensaver]
picture-uri='file:///usr/share/backgrounds/auraos/wallpaper.jpg'
EOF

# Compile GNOME settings schemas inside chroot
chroot "${ROOT_DIR}" glib-compile-schemas /usr/share/glib-2.0/schemas

# 8. Copy Custom fastfetch configuration and ASCII logo
echo "Configuring fastfetch..."
mkdir -p "${ROOT_DIR}/etc/fastfetch"
mkdir -p "${ROOT_DIR}/etc/auraos"
cp ../config/fastfetch/config.jsonc "${ROOT_DIR}/etc/fastfetch/config.jsonc"
cp ../config/fastfetch/ascii_logo.txt "${ROOT_DIR}/etc/auraos/ascii_logo.txt"

# Override /etc/os-release and /usr/lib/os-release to brand OS as AuraOS
cat <<OSRELEASE > "${ROOT_DIR}/etc/os-release"
NAME="AuraOS"
VERSION="26.04 LTS (Resolute Raccoon)"
ID=auraos
ID_LIKE=ubuntu
PRETTY_NAME="AuraOS 26.04 LTS"
VERSION_ID="26.04"
HOME_URL="https://github.com/nibir-ai/AuraOs"
SUPPORT_URL="https://github.com/nibir-ai/AuraOs/issues"
BUG_REPORT_URL="https://github.com/nibir-ai/AuraOs/issues"
PRIVACY_POLICY_URL="https://github.com/nibir-ai/AuraOs"
VERSION_CODENAME=resolute
UBUNTU_CODENAME=resolute
OSRELEASE

cp "${ROOT_DIR}/etc/os-release" "${ROOT_DIR}/usr/lib/os-release"

# 9. Inject Custom Installer
echo "Injecting custom installer..."
# Copy the compiled aura-installer binary to the chroot
cp ../build/aura-installer "${ROOT_DIR}/usr/bin/aura-installer" || echo "Warning: aura-installer binary not found in ../build/. Ensure it is compiled first."

# Configure live session user autostart for the custom installer
mkdir -p "${ROOT_DIR}/etc/xdg/autostart"
cat <<AUTOSTART > "${ROOT_DIR}/etc/xdg/autostart/aura-installer.desktop"
[Desktop Entry]
Type=Application
Name=AuraOS Installer
Comment=Install AuraOS to your hard drive
Exec=/usr/bin/aura-installer
Icon=system-software-install
Categories=System;
OnlyShowIn=GNOME;
Terminal=false
AUTOSTART

# Create live user configuration
cat <<'EOF' > "${ROOT_DIR}/tmp/create-live-user.sh"
#!/bin/bash
if ! id "live" &>/dev/null; then
    useradd -m -s /bin/bash -g auraos-users live || useradd -m -s /bin/bash live
    passwd -d live
    echo "live ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers
fi
EOF
chmod +x "${ROOT_DIR}/tmp/create-live-user.sh"
chroot "${ROOT_DIR}" /tmp/create-live-user.sh
rm "${ROOT_DIR}/tmp/create-live-user.sh"

# 10. Create SquashFS filesystem
echo "Generating SquashFS image..."
mkdir -p "${IMAGE_DIR}/live"
mksquashfs "${ROOT_DIR}" "${IMAGE_DIR}/live/filesystem.squashfs" -noappend -comp xz

# 11. Extract kernel and initramfs for booting
echo "Extracting kernel files..."
cp "${ROOT_DIR}/boot/vmlinuz-"* "${IMAGE_DIR}/live/vmlinuz"
cp "${ROOT_DIR}/boot/initrd.img-"* "${IMAGE_DIR}/live/initrd"

# 12. Configure isolinux/GRUB boot options (Aura kernel branding)
echo "Configuring bootloader files..."
mkdir -p "${IMAGE_DIR}/isolinux"
cp /usr/lib/ISOLINUX/isolinux.bin "${IMAGE_DIR}/isolinux/"
cp /usr/lib/syslinux/modules/bios/ldlinux.c32 "${IMAGE_DIR}/isolinux/"

cat <<EOF > "${IMAGE_DIR}/isolinux/isolinux.cfg"
default live
label live
  menu label ^Start AuraOS with Aura kernel (Live Session)
  kernel /live/vmlinuz
  append initrd=/live/initrd boot=live quiet splash ---
EOF

# Set up EFI boot configuration (Aura kernel branding)
mkdir -p "${IMAGE_DIR}/boot/grub"
cat <<EOF > "${IMAGE_DIR}/boot/grub/grub.cfg"
menuentry "Start AuraOS with Aura kernel (Live Session)" {
    set gfxpayload=keep
    linux /live/vmlinuz boot=live quiet splash ---
    initrd /live/initrd
}
EOF

# 13. Generate final bootable ISO image
echo "Building final bootable hybrid ISO..."
xorriso -as mkisofs \
    -iso-level 3 \
    -full-iso9660-filenames \
    -volid "AURAOS_LIVE" \
    -eltorito-boot isolinux/isolinux.bin \
    -eltorito-catalog isolinux/boot.cat \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    -isohybrid-mbr /usr/lib/syslinux/mbr.bin \
    -eltorito-alt-boot \
    -e boot/grub/efi.img \
    -no-emul-boot -isohybrid-gpt-basdat \
    -output "../${ISO_NAME}" \
    "${IMAGE_DIR}"

echo "ISO compilation complete! Output written to: ${ISO_NAME}"
