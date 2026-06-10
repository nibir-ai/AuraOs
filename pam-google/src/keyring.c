/*
 * keyring.c — GNOME Keyring Integration for AuraOS PAM
 *
 * Manages storing and retrieving OAuth tokens in GNOME Keyring
 * (via libsecret D-Bus API) and caching access tokens in the
 * Linux kernel keyring for in-session performance.
 *
 * Copyright (C) 2025 AuraOS Contributors
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

#include <libsecret/secret.h>
#include <string.h>
#include <stdlib.h>
#include <syslog.h>
#include <sys/types.h>
#include <keyutils.h>

#include "pam_google.h"

/* ─── libsecret Schema Definition ──────────────────────────────────── */

/*
 * Schema for AuraOS Google Account tokens.
 * Stored in the user's default GNOME Keyring collection.
 *
 * Attributes:
 *   account_id:  Google user ID (sub claim)
 *   token_type:  "refresh" or "access"
 */
static const SecretSchema AURAOS_TOKEN_SCHEMA = {
    "org.auraos.GoogleAccount",
    SECRET_SCHEMA_NONE,
    {
        { "account_id", SECRET_SCHEMA_ATTRIBUTE_STRING },
        { "token_type", SECRET_SCHEMA_ATTRIBUTE_STRING },
        { NULL, 0 }
    }
};

/* ─── Refresh Token: Get from GNOME Keyring ────────────────────────── */

int auraos_keyring_get_refresh_token(const char *sub,
                                      char *out_token,
                                      size_t token_len) {
    if (!sub || !out_token || token_len == 0) {
        return -1;
    }

    GError *error = NULL;

    /*
     * Synchronous lookup — this blocks, which is acceptable for PAM
     * (PAM modules run in the auth pipeline, blocking is expected).
     *
     * The token is stored with attributes:
     *   account_id = sub
     *   token_type = "refresh"
     */
    gchar *secret = secret_password_lookup_sync(
        &AURAOS_TOKEN_SCHEMA,
        NULL,   /* cancellable */
        &error,
        "account_id", sub,
        "token_type", "refresh",
        NULL
    );

    if (error) {
        auraos_log(LOG_ERR,
                   "pam_google: keyring lookup error for sub '%s': %s",
                   sub, error->message);
        g_error_free(error);
        return -1;
    }

    if (!secret) {
        auraos_log(LOG_WARNING,
                   "pam_google: no refresh token found in keyring for sub '%s'",
                   sub);
        return -1;
    }

    strncpy(out_token, secret, token_len - 1);
    out_token[token_len - 1] = '\0';

    /* Free the secret using libsecret's secure free (zeroes memory) */
    secret_password_free(secret);

    return 0;
}

/* ─── Refresh Token: Store in GNOME Keyring ────────────────────────── */

int auraos_keyring_store_refresh_token(const char *sub,
                                        const char *refresh_token) {
    if (!sub || !refresh_token) {
        return -1;
    }

    GError *error = NULL;

    /*
     * Store the refresh token in the default keyring.
     * The label is human-readable and shown in Seahorse/GNOME Passwords.
     */
    char label[256];
    snprintf(label, sizeof(label), "AuraOS Google Account (%s)", sub);

    gboolean stored = secret_password_store_sync(
        &AURAOS_TOKEN_SCHEMA,
        SECRET_COLLECTION_DEFAULT,
        label,
        refresh_token,
        NULL,   /* cancellable */
        &error,
        "account_id", sub,
        "token_type", "refresh",
        NULL
    );

    if (error) {
        auraos_log(LOG_ERR,
                   "pam_google: keyring store error for sub '%s': %s",
                   sub, error->message);
        g_error_free(error);
        return -1;
    }

    if (!stored) {
        auraos_log(LOG_ERR,
                   "pam_google: failed to store refresh token for sub '%s'", sub);
        return -1;
    }

    auraos_log(LOG_INFO,
               "pam_google: refresh token stored/updated in keyring for sub '%s'",
               sub);

    return 0;
}

/* ─── Access Token: Cache in Kernel Keyring ────────────────────────── */

/*
 * The kernel keyring provides a fast, in-kernel credential store that
 * doesn't require D-Bus. Access tokens are cached here for performance
 * during the session. They are NOT visible in /proc/*/environ.
 *
 * The keyring entry is created in the user's session keyring and
 * automatically expires after ttl_secs.
 */
int auraos_keyring_cache_access_token(const char *sub,
                                       const char *access_token,
                                       int ttl_secs) {
    if (!sub || !access_token || ttl_secs <= 0) {
        return -1;
    }

    /*
     * Key name format: auraos:access_token:<sub>
     * Stored in the user session keyring.
     */
    char key_name[512];
    snprintf(key_name, sizeof(key_name), "auraos:access_token:%s", sub);

    /* Add or update the key in the user session keyring */
    key_serial_t key = add_key(
        "user",                               /* key type */
        key_name,                             /* key description */
        access_token,                         /* payload */
        strlen(access_token),                 /* payload length */
        KEY_SPEC_USER_SESSION_KEYRING         /* session keyring */
    );

    if (key < 0) {
        auraos_log(LOG_ERR,
                   "pam_google: failed to cache access token in kernel keyring: %m");
        return -1;
    }

    /* Set expiry on the key */
    if (keyctl_set_timeout(key, (unsigned int)ttl_secs) < 0) {
        auraos_log(LOG_WARNING,
                   "pam_google: failed to set kernel keyring TTL: %m");
        /* Non-fatal — token will persist until session ends */
    }

    return 0;
}
