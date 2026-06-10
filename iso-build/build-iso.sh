#!/usr/bin/env bash
# build-iso.sh — AuraOS ISO Generation Script
#
# Bootstraps an Ubuntu 24.04 LTS chroot, injects custom configurations and
# compiled AuraOS Debian packages, and outputs a bootable hybrid ISO image.
#
# Copyright (C) 2025 AuraOS Contributors
# SPDX-License-Identifier: GPL-3.0-or-later

set -euo pipefail

# Configuration
CODENAME="noble"
ARCH="amd64"
ROOT_DIR="chroot"
IMAGE_DIR="image"
ISO_NAME="auraos-24.04-desktop-${ARCH}.iso"
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

# 4. Configure chroot packages and repositories
echo "Configuring apt repositories inside chroot..."
cat <<EOF > "${ROOT_DIR}/etc/apt/sources.list"
deb http://archive.ubuntu.com/ubuntu/ noble main restricted universe multiverse
deb http://archive.ubuntu.com/ubuntu/ noble-updates main restricted universe multiverse
deb http://archive.ubuntu.com/ubuntu/ noble-security main restricted universe multiverse
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
    calamares \
    calamares-settings-ubuntu

# Enable systemd services
systemctl enable gdm3
systemctl enable NetworkManager
systemctl enable systemd-resolved

# Configure GDM automatic login / autostart
mkdir -p /etc/gdm3
cat <<GDM > /etc/gdm3/custom.conf
[daemon]
AutomaticLoginEnable=false
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

# 7. Configure GNOME Shell extension defaults
echo "Setting GNOME default overrides..."
mkdir -p "${ROOT_DIR}/usr/share/glib-2.0/schemas"
cat <<EOF > "${ROOT_DIR}/usr/share/glib-2.0/schemas/99-auraos-defaults.gschema.override"
[org.gnome.shell]
enabled-extensions=['gemini-assistant@auraos.org', 'aura-account-switcher']

[org.gnome.desktop.interface]
enable-hot-corners=false
EOF
chroot "${ROOT_DIR}" glib-compile-schemas /usr/share/glib-2.0/schemas

# 8. Create SquashFS filesystem
echo "Generating SquashFS image..."
mkdir -p "${IMAGE_DIR}/live"
mksquashfs "${ROOT_DIR}" "${IMAGE_DIR}/live/filesystem.squashfs" -noappend -comp xz

# 9. Extract kernel and initramfs for booting
echo "Extracting kernel files..."
cp "${ROOT_DIR}/boot/vmlinuz-"* "${IMAGE_DIR}/live/vmlinuz"
cp "${ROOT_DIR}/boot/initrd.img-"* "${IMAGE_DIR}/live/initrd"

# 10. Configure isolinux/GRUB boot options
echo "Configuring bootloader files..."
mkdir -p "${IMAGE_DIR}/isolinux"
cp /usr/lib/ISOLINUX/isolinux.bin "${IMAGE_DIR}/isolinux/"
cp /usr/lib/syslinux/modules/bios/ldlinux.c32 "${IMAGE_DIR}/isolinux/"

cat <<EOF > "${IMAGE_DIR}/isolinux/isolinux.cfg"
default live
label live
  menu label ^Start AuraOS (Live Session)
  kernel /live/vmlinuz
  append initrd=/live/initrd boot=live quiet splash ---
EOF

# Set up EFI boot configuration
mkdir -p "${IMAGE_DIR}/boot/grub"
cat <<EOF > "${IMAGE_DIR}/boot/grub/grub.cfg"
menuentry "Start AuraOS 24.04 (Live Session)" {
    set gfxpayload=keep
    linux /live/vmlinuz boot=live quiet splash ---
    initrd /live/initrd
}
EOF

# 11. Generate final bootable ISO image
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
EOF
