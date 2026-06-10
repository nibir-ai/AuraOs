# main.py — Calamares Job Module for Google Sign-In on Install
#
# Launches aura-auth-helper in the installer context to configure
# the Google identity profile before first boot.
#
# Copyright (C) 2025 AuraOS Contributors
# SPDX-License-Identifier: GPL-3.0-or-later

import libcalamares
import subprocess
import os

def run():
    """
    Runs the Google OAuth sign-in job.
    Retrieves the root mount point of the new installation, launches the PKCE flow helper,
    and maps the user account details directly into the target environment.
    """
    root_mount_point = libcalamares.globalstorage.value("rootMountPoint")
    if not root_mount_point:
        libcalamares.utils.warning("No installation target mount point found.")
        return 1

    libcalamares.utils.debug(f"Target system mounted at: {root_mount_point}")

    # Set up host environmental variables so the GTK WebView window appears on the live ISO display
    env = os.environ.copy()
    env["DISPLAY"] = env.get("DISPLAY", ":0")
    env["XAUTHORITY"] = env.get("XAUTHORITY", "/run/user/999/gdm/Xauthority") # Adjust for live user

    # Create target accounts database path
    target_accounts_db = os.path.join(root_mount_point, "var/lib/auraos/accounts")
    os.makedirs(target_accounts_db, exist_ok=True)

    # Launch aura-auth-helper on the host live environment, but write to target account database
    libcalamares.utils.debug("Launching Google Sign-In helper...")
    try:
        # Run auth helper on live system targeting the install path
        process = subprocess.run([
            "/usr/lib/auraos/aura-auth-helper",
            "--account-db", target_accounts_db
        ], env=env, capture_output=True, text=True)

        if process.returncode != 0:
            libcalamares.utils.warning(f"Google Sign-In failed: {process.stderr}")
            return 1
            
        libcalamares.utils.debug("Google login completed. Syncing user into target /etc/passwd...")
        
        # Now, sync the newly created user from the host /etc/passwd into target /etc/passwd
        # The easiest way is to re-run aura-auth-helper inside the chroot, referencing the cached metadata
        # so it creates the user account internally on the target disk.
        chroot_cmd = [
            "chroot", root_mount_point,
            "/usr/lib/auraos/aura-auth-helper",
            "--sync-local" # Trigger only account creation based on existing JSON in /var/lib/auraos/accounts/
        ]
        
        chroot_process = subprocess.run(chroot_cmd, capture_output=True, text=True)
        if chroot_process.returncode != 0:
             libcalamares.utils.warning(f"Chroot account synchronization failed: {chroot_process.stderr}")
             # Non-fatal; the user can create account on first login, but warning is outputted
        
        return None

    except Exception as e:
        libcalamares.utils.warning(f"An unexpected error occurred during Google sign-in: {str(e)}")
        return 1
