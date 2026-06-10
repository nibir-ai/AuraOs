// install.rs — Low-Level Installation Worker Tasks for AuraOS
//
// Performs disk wiping, partitioning, formatting, root filesystem copying (rsync),
// chroot mounting, GRUB bootloader installation, and Google account user seeding.
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tracing::{info, warn};

pub struct InstallConfig {
    pub disk: String, // e.g. "/dev/sda"
}

pub async fn run_install<F>(config: InstallConfig, progress_callback: F) -> Result<()>
where
    F: Fn(f64, &str) + Send + 'static,
{
    let disk = config.disk;
    let target_mount = "/mnt";

    // 1. Wiping and partitioning the target disk (GPT, 512MB EFI + Ext4 remaining)
    progress_callback(0.10, "Partitioning disk...");
    info!("Partitioning disk {}", disk);

    run_cmd("parted", &["-s", &disk, "mklabel", "gpt"])?;
    run_cmd("parted", &["-s", &disk, "mkpart", "ESP", "fat32", "1MiB", "513MiB"])?;
    run_cmd("parted", &["-s", &disk, "set", "1", "esp", "on"])?;
    run_cmd("parted", &["-s", &disk, "mkpart", "primary", "ext4", "513MiB", "100%"])?;

    // Determine partitions
    let (efi_part, root_part) = if disk.contains("nvme") || disk.contains("mmcblk") {
        (format!("{}p1", disk), format!("{}p2", disk))
    } else {
        (format!("{}1", disk), format!("{}2", disk))
    };

    // 2. Formatting filesystems
    progress_callback(0.20, "Formatting filesystems...");
    info!("Formatting partitions: efi={}, root={}", efi_part, root_part);

    run_cmd("mkfs.vfat", &["-F32", &efi_part])?;
    run_cmd("mkfs.ext4", &["-F", &root_part])?;

    // 3. Mounting filesystems to /mnt
    progress_callback(0.30, "Mounting target filesystem...");
    info!("Mounting target partition to {}", target_mount);

    // Unmount just in case
    let _ = run_cmd("umount", &["-R", target_mount]);

    run_cmd("mount", &[&root_part, target_mount])?;
    fs::create_dir_all(format!("{}/boot/efi", target_mount))?;
    run_cmd("mount", &[&efi_part, format!("{}/boot/efi", target_mount)])?;

    // 4. Extracting live root filesystem to target mount
    progress_callback(0.40, "Copying system files (this may take a few minutes)...");
    info!("Rsyncing live root filesystem to target");

    // Standard live system copying parameters
    let rsync_args = [
        "-aAXv",
        "--exclude=/dev/*",
        "--exclude=/proc/*",
        "--exclude=/sys/*",
        "--exclude=/tmp/*",
        "--exclude=/run/*",
        "--exclude=/mnt/*",
        "--exclude=/media/*",
        "--exclude=/lost+found",
        "/",
        target_mount,
    ];
    run_cmd("rsync", &rsync_args)?;

    // 5. Creating /etc/fstab with UUIDs
    progress_callback(0.70, "Configuring mount filesystems (fstab)...");
    info!("Generating fstab for target disk");

    let efi_uuid = get_blkid_uuid(&efi_part)?;
    let root_uuid = get_blkid_uuid(&root_part)?;

    let fstab_content = format!(
        "# /etc/fstab: static file system information.\n\
         # <file system> <mount point>   <type>  <options>       <dump>  <pass>\n\
         UUID={}  /               ext4    errors=remount-ro  0       1\n\
         UUID={}  /boot/efi       vfat    umask=0077          0       2\n",
        root_uuid, efi_uuid
    );

    let fstab_path = format!("{}/etc/fstab", target_mount);
    fs::write(&fstab_path, fstab_content).context("Failed to write target fstab")?;

    // 6. Installing Bootloader (GRUB)
    progress_callback(0.80, "Installing GRUB bootloader...");
    info!("Chrooting to mount virtual devices and execute grub-install");

    // Mount virtual device filesystems
    run_cmd("mount", &["--bind", "/dev", &format!("{}/dev", target_mount)])?;
    run_cmd("mount", &["--bind", "/proc", &format!("{}/proc", target_mount)])?;
    run_cmd("mount", &["--bind", "/sys", &format!("{}/sys", target_mount)])?;

    // Install GRUB
    run_cmd("chroot", &[target_mount, "grub-install", "--target=x86_64-efi", "--efi-directory=/boot/efi", "--bootloader-id=AuraOS", "--recheck"])?;
    run_cmd("chroot", &[target_mount, "update-grub"])?;

    // 7. Seed Google Account metadata and sync user
    progress_callback(0.90, "Configuring Google user credentials...");
    info!("Copying accounts and running sync-local inside chroot");

    let live_acc_db = "/var/lib/auraos/accounts";
    let target_acc_db = format!("{}/var/lib/auraos/accounts", target_mount);

    if Path::new(live_acc_db).exists() {
        fs::create_dir_all(&target_acc_db).ok();
        // Copy accounts
        let dir = fs::read_dir(live_acc_db)?;
        for entry in dir {
            let entry = entry?;
            let dest = format!("{}/{}", target_acc_db, entry.file_name().to_string_lossy());
            fs::copy(entry.path(), dest)?;
        }
    }

    // Run account sync-local to pre-add accounts inside target /etc/passwd
    let sync_res = run_cmd("chroot", &[target_mount, "/usr/lib/auraos/aura-auth-helper", "--sync-local"]);
    if let Err(e) = sync_res {
        warn!("Chroot account sync failed: {}. Account will be created on first boot.", e);
    }

    // Unmount virtual bind mounts
    let _ = run_cmd("umount", &[&format!("{}/dev", target_mount)]);
    let _ = run_cmd("umount", &[&format!("{}/proc", target_mount)]);
    let _ = run_cmd("umount", &[&format!("{}/sys", target_mount)]);

    // Unmount target filesystems
    let _ = run_cmd("umount", &["-R", target_mount]);

    progress_callback(1.0, "Installation finished! Please restart your PC.");
    info!("Installation complete.");
    Ok(())
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .context(format!("Failed to execute command: {}", cmd))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Command '{} {:?}' failed: {}", cmd, args, stderr);
    }
    Ok(())
}

fn get_blkid_uuid(partition: &str) -> Result<String> {
    let output = Command::new("blkid")
        .args(["-s", "UUID", "-o", "value", partition])
        .output()
        .context("Failed to run blkid")?;

    if !output.status.success() {
        anyhow::bail!("blkid failed for partition {}", partition);
    }

    let uuid = String::from_utf8(output.stdout)?
        .trim()
        .to_string();

    if uuid.is_empty() {
        anyhow::bail!("Could not retrieve UUID for partition {}", partition);
    }
    Ok(uuid)
}
