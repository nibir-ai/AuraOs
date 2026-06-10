/*
 * pam_google.h — AuraOS Google Authentication PAM Module
 *
 * This header defines the internal interfaces used by pam_google.so to
 * authenticate Linux users against their Google account via OAuth 2.0
 * token validation.
 *
 * Copyright (C) 2025 AuraOS Contributors
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

#ifndef PAM_GOOGLE_H
#define PAM_GOOGLE_H

#include <security/pam_modules.h>
#include <stdbool.h>
#include <stddef.h>

/* ─── Configuration Constants ──────────────────────────────────────── */

/* Default paths */
#define AURAOS_ACCOUNT_DB_PATH    "/var/lib/auraos/accounts"
#define AURAOS_TOKEN_LOG_PATH     "/var/log/auraos/token-lifecycle.log"
#define AURAOS_CONFIG_PATH        "/etc/auraos/config.toml"

/* OAuth endpoints */
#define GOOGLE_TOKEN_ENDPOINT     "https://oauth2.googleapis.com/token"
#define GOOGLE_TOKENINFO_ENDPOINT "https://oauth2.googleapis.com/tokeninfo"

/* Embedded OAuth client credentials (obfuscated at compile time) */
/* These are placeholders — replace with actual registered client credentials */
#define AURAOS_CLIENT_ID          "AURAOS_CLIENT_ID_PLACEHOLDER.apps.googleusercontent.com"
#define AURAOS_CLIENT_SECRET      "AURAOS_CLIENT_SECRET_PLACEHOLDER"

/* Token refresh parameters */
#define TOKEN_EXPIRY_BUFFER_SECS  300   /* Refresh 5 minutes before expiry */
#define MAX_TOKEN_REFRESH_RETRIES 3
#define TOKEN_REFRESH_TIMEOUT_MS  10000 /* 10 second HTTP timeout */

/* Offline auth parameters */
#define OFFLINE_PIN_MIN_LENGTH    6
#define OFFLINE_PIN_MAX_LENGTH    32
#define BCRYPT_WORK_FACTOR        12

/* ─── Data Structures ──────────────────────────────────────────────── */

/**
 * auraos_account_t — Represents a stored AuraOS user account.
 * Loaded from /var/lib/auraos/accounts/<sub>.json
 */
typedef struct {
    char sub[256];               /* Google user ID (sub claim) */
    char email[256];             /* Google email address */
    char display_name[256];      /* User's display name */
    char avatar_url[512];        /* Profile photo URL */
    char linux_username[64];     /* Mapped Linux username */
    char scopes_granted[2048];   /* Comma-separated OAuth scopes */
    char account_created[64];    /* ISO 8601 timestamp */
    char offline_pin_hash[128];  /* bcrypt hash of offline PIN */
} auraos_account_t;

/**
 * token_response_t — OAuth token exchange/refresh response.
 */
typedef struct {
    char access_token[2048];     /* Short-lived access token */
    char refresh_token[2048];    /* Long-lived refresh token (may be rotated) */
    char id_token[4096];         /* JWT with user profile claims */
    int  expires_in;             /* Seconds until access_token expiry */
    char scope[2048];            /* Granted scopes */
    bool has_new_refresh_token;  /* True if Google rotated the refresh token */
} token_response_t;

/**
 * pam_google_opts_t — Options parsed from PAM configuration.
 */
typedef struct {
    char account_db[512];        /* Path to account database directory */
    bool refresh_tokens;         /* Whether to refresh tokens on session open */
    bool debug;                  /* Enable debug logging to syslog */
} pam_google_opts_t;

/* ─── Token Refresh (token_refresh.c) ──────────────────────────────── */

/**
 * Refresh the OAuth access token using the stored refresh token.
 *
 * @param refresh_token  The stored refresh token from gnome-keyring.
 * @param response       Output: populated with new tokens on success.
 * @return 0 on success, -1 on network error, -2 on token revoked (400).
 */
int auraos_refresh_token(const char *refresh_token, token_response_t *response);

/**
 * Validate an access token against Google's tokeninfo endpoint.
 *
 * @param access_token  The access token to validate.
 * @param out_sub       Output: Google user ID (sub claim) if valid.
 * @param sub_len       Length of out_sub buffer.
 * @return 0 if valid, -1 if invalid/expired.
 */
int auraos_validate_token(const char *access_token, char *out_sub, size_t sub_len);

/* ─── Account Lookup (account_lookup.c) ─────────────────────────────── */

/**
 * Look up an AuraOS account by Linux username.
 *
 * @param account_db_path  Path to the accounts directory.
 * @param username         Linux username to look up.
 * @param account          Output: populated account struct.
 * @return 0 on success, -1 if not found.
 */
int auraos_account_lookup_by_username(const char *account_db_path,
                                       const char *username,
                                       auraos_account_t *account);

/**
 * Look up an AuraOS account by Google sub (user ID).
 *
 * @param account_db_path  Path to the accounts directory.
 * @param sub              Google user ID.
 * @param account          Output: populated account struct.
 * @return 0 on success, -1 if not found.
 */
int auraos_account_lookup_by_sub(const char *account_db_path,
                                  const char *sub,
                                  auraos_account_t *account);

/**
 * Write/update an account record to disk.
 *
 * @param account_db_path  Path to the accounts directory.
 * @param account          Account data to write.
 * @return 0 on success, -1 on error.
 */
int auraos_account_save(const char *account_db_path,
                         const auraos_account_t *account);

/* ─── Keyring Integration (keyring.c) ──────────────────────────────── */

/**
 * Retrieve the refresh token for a user from GNOME Keyring.
 *
 * @param sub            Google user ID (used as keyring attribute).
 * @param out_token      Output: refresh token string.
 * @param token_len      Length of out_token buffer.
 * @return 0 on success, -1 on error (keyring locked, not found, etc.)
 */
int auraos_keyring_get_refresh_token(const char *sub,
                                      char *out_token,
                                      size_t token_len);

/**
 * Store or update the refresh token in GNOME Keyring.
 *
 * @param sub            Google user ID.
 * @param refresh_token  The refresh token to store.
 * @return 0 on success, -1 on error.
 */
int auraos_keyring_store_refresh_token(const char *sub,
                                        const char *refresh_token);

/**
 * Store the access token in the kernel keyring (tmpfs-backed, session-scoped).
 *
 * @param sub            Google user ID.
 * @param access_token   The access token to cache.
 * @param ttl_secs       Time-to-live in seconds.
 * @return 0 on success, -1 on error.
 */
int auraos_keyring_cache_access_token(const char *sub,
                                       const char *access_token,
                                       int ttl_secs);

/* ─── Offline Authentication (offline_auth.c) ──────────────────────── */

/**
 * Verify the user's offline PIN against the stored bcrypt hash.
 *
 * @param pin            User-entered PIN.
 * @param stored_hash    bcrypt hash from the account record.
 * @return true if PIN matches, false otherwise.
 */
bool auraos_offline_verify_pin(const char *pin, const char *stored_hash);

/**
 * Hash a new PIN for storage using bcrypt.
 *
 * @param pin            The PIN to hash.
 * @param out_hash       Output: bcrypt hash string.
 * @param hash_len       Length of out_hash buffer.
 * @return 0 on success, -1 on error.
 */
int auraos_offline_hash_pin(const char *pin, char *out_hash, size_t hash_len);

/**
 * Check if offline authentication is available for this user.
 * (Returns true if an offline PIN hash is set in the account record.)
 *
 * @param account  The user's account record.
 * @return true if offline PIN is configured.
 */
bool auraos_offline_is_available(const auraos_account_t *account);

/* ─── Utility ──────────────────────────────────────────────────────── */

/**
 * Parse PAM module options from the PAM configuration line.
 *
 * @param argc   Number of option arguments.
 * @param argv   Option argument strings.
 * @param opts   Output: parsed options.
 */
void auraos_parse_pam_opts(int argc, const char **argv, pam_google_opts_t *opts);

/**
 * Log a message to syslog with the "pam_google" facility.
 */
void auraos_log(int priority, const char *fmt, ...);

#endif /* PAM_GOOGLE_H */
