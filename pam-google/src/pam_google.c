/*
 * pam_google.c — AuraOS Google Authentication PAM Module
 *
 * This PAM module authenticates Linux users against their Google account
 * by validating/refreshing OAuth 2.0 tokens stored in GNOME Keyring.
 *
 * PAM configuration (/etc/pam.d/aura-gdm-password):
 *   auth    required    pam_google.so  account_db=/var/lib/auraos/accounts
 *   session required    pam_google.so  refresh_tokens
 *
 * Copyright (C) 2025 AuraOS Contributors
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

#define PAM_SM_AUTH
#define PAM_SM_SESSION
#define PAM_SM_ACCOUNT

#include <security/pam_modules.h>
#include <security/pam_ext.h>
#include <syslog.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <unistd.h>
#include <errno.h>
#include <stdarg.h>

#include "pam_google.h"

/* ─── Logging ──────────────────────────────────────────────────────── */

void auraos_log(int priority, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vsyslog(priority, fmt, ap);
    va_end(ap);
}

/* ─── PAM Option Parsing ──────────────────────────────────────────── */

void auraos_parse_pam_opts(int argc, const char **argv, pam_google_opts_t *opts) {
    /* Set defaults */
    strncpy(opts->account_db, AURAOS_ACCOUNT_DB_PATH, sizeof(opts->account_db) - 1);
    opts->refresh_tokens = false;
    opts->debug = false;

    for (int i = 0; i < argc; i++) {
        if (strncmp(argv[i], "account_db=", 11) == 0) {
            strncpy(opts->account_db, argv[i] + 11, sizeof(opts->account_db) - 1);
        } else if (strcmp(argv[i], "refresh_tokens") == 0) {
            opts->refresh_tokens = true;
        } else if (strcmp(argv[i], "debug") == 0) {
            opts->debug = true;
        }
    }
}

/* ─── pam_sm_authenticate ──────────────────────────────────────────── */
/*
 * Called during the authentication phase. Validates that the user has a
 * valid Google account and that their refresh token is still active.
 *
 * Flow:
 * 1. Get the username from PAM
 * 2. Look up the AuraOS account record
 * 3. Retrieve the refresh token from GNOME Keyring
 * 4. Attempt to refresh the access token via Google's token endpoint
 * 5. If token refresh succeeds → PAM_SUCCESS
 * 6. If token is revoked → attempt offline PIN auth
 * 7. If all fails → PAM_AUTH_ERR
 */
PAM_EXTERN int pam_sm_authenticate(pam_handle_t *pamh, int flags,
                                    int argc, const char **argv) {
    pam_google_opts_t opts;
    auraos_parse_pam_opts(argc, argv, &opts);

    const char *username = NULL;
    int ret;

    /* Step 1: Get the username */
    ret = pam_get_user(pamh, &username, "AuraOS login: ");
    if (ret != PAM_SUCCESS || username == NULL) {
        auraos_log(LOG_ERR, "pam_google: failed to get username: %s",
                   pam_strerror(pamh, ret));
        return PAM_AUTH_ERR;
    }

    if (opts.debug) {
        auraos_log(LOG_DEBUG, "pam_google: authenticating user '%s'", username);
    }

    /* Step 2: Look up the AuraOS account */
    auraos_account_t account;
    memset(&account, 0, sizeof(account));

    ret = auraos_account_lookup_by_username(opts.account_db, username, &account);
    if (ret != 0) {
        auraos_log(LOG_WARNING, "pam_google: no AuraOS account found for user '%s'",
                   username);
        /* Not an AuraOS-managed user — let other PAM modules handle it */
        return PAM_USER_UNKNOWN;
    }

    if (opts.debug) {
        auraos_log(LOG_DEBUG, "pam_google: found account for '%s' (sub=%s, email=%s)",
                   username, account.sub, account.email);
    }

    /* Step 3: Retrieve refresh token from GNOME Keyring */
    char refresh_token[2048];
    memset(refresh_token, 0, sizeof(refresh_token));

    ret = auraos_keyring_get_refresh_token(account.sub, refresh_token, sizeof(refresh_token));
    if (ret != 0) {
        auraos_log(LOG_WARNING,
                   "pam_google: failed to retrieve refresh token for '%s' from keyring",
                   username);
        /* Fall through to offline auth */
        goto try_offline;
    }

    /* Step 4: Attempt token refresh */
    token_response_t token_resp;
    memset(&token_resp, 0, sizeof(token_resp));

    ret = auraos_refresh_token(refresh_token, &token_resp);
    if (ret == 0) {
        /* Success — cache the new access token */
        auraos_keyring_cache_access_token(account.sub, token_resp.access_token,
                                           token_resp.expires_in);

        /* If Google rotated the refresh token, update the stored one */
        if (token_resp.has_new_refresh_token) {
            auraos_keyring_store_refresh_token(account.sub, token_resp.refresh_token);
            auraos_log(LOG_INFO, "pam_google: refresh token rotated for '%s'", username);
        }

        /* Store the access token in PAM data for session phase */
        char *token_copy = strdup(token_resp.access_token);
        if (token_copy) {
            pam_set_data(pamh, "auraos_access_token", token_copy, NULL);
        }

        if (opts.debug) {
            auraos_log(LOG_DEBUG, "pam_google: online auth successful for '%s'", username);
        }

        /* Clear sensitive data from stack */
        memset(&token_resp, 0, sizeof(token_resp));
        memset(refresh_token, 0, sizeof(refresh_token));

        return PAM_SUCCESS;
    }

    if (ret == -2) {
        /* Token revoked by Google (HTTP 400) */
        auraos_log(LOG_WARNING,
                   "pam_google: refresh token revoked for '%s', trying offline auth",
                   username);
    } else {
        /* Network error — try offline */
        auraos_log(LOG_WARNING,
                   "pam_google: network error refreshing token for '%s', trying offline",
                   username);
    }

try_offline:
    /* Step 5: Offline authentication via PIN */
    memset(refresh_token, 0, sizeof(refresh_token));

    if (!auraos_offline_is_available(&account)) {
        auraos_log(LOG_ERR,
                   "pam_google: no offline PIN configured for '%s' — auth failed",
                   username);
        return PAM_AUTH_ERR;
    }

    /* Prompt for offline PIN */
    const char *pin = NULL;
    ret = pam_get_authtok(pamh, PAM_AUTHTOK, &pin, "AuraOS offline PIN: ");
    if (ret != PAM_SUCCESS || pin == NULL) {
        auraos_log(LOG_ERR, "pam_google: failed to read offline PIN");
        return PAM_AUTH_ERR;
    }

    if (auraos_offline_verify_pin(pin, account.offline_pin_hash)) {
        auraos_log(LOG_INFO,
                   "pam_google: offline PIN auth successful for '%s'", username);
        return PAM_SUCCESS;
    }

    auraos_log(LOG_WARNING,
               "pam_google: offline PIN verification failed for '%s'", username);
    return PAM_AUTH_ERR;
}

/* ─── pam_sm_setcred ───────────────────────────────────────────────── */
/*
 * Called to set user credentials. We use this to export the access token
 * into the kernel keyring (not environment variables, for security).
 */
PAM_EXTERN int pam_sm_setcred(pam_handle_t *pamh, int flags,
                               int argc, const char **argv) {
    /* Credentials are handled in the session phase */
    return PAM_SUCCESS;
}

/* ─── pam_sm_acct_mgmt ─────────────────────────────────────────────── */
/*
 * Account management: check if the Google account is still active.
 */
PAM_EXTERN int pam_sm_acct_mgmt(pam_handle_t *pamh, int flags,
                                 int argc, const char **argv) {
    const char *username = NULL;
    int ret = pam_get_user(pamh, &username, NULL);
    if (ret != PAM_SUCCESS) {
        return PAM_AUTH_ERR;
    }

    pam_google_opts_t opts;
    auraos_parse_pam_opts(argc, argv, &opts);

    auraos_account_t account;
    ret = auraos_account_lookup_by_username(opts.account_db, username, &account);
    if (ret != 0) {
        return PAM_USER_UNKNOWN;
    }

    /* Account exists and is mapped — allow */
    return PAM_SUCCESS;
}

/* ─── pam_sm_open_session ──────────────────────────────────────────── */
/*
 * Called when a user session is opened. If refresh_tokens is set,
 * ensures the access token is fresh and available for the session.
 */
PAM_EXTERN int pam_sm_open_session(pam_handle_t *pamh, int flags,
                                    int argc, const char **argv) {
    pam_google_opts_t opts;
    auraos_parse_pam_opts(argc, argv, &opts);

    if (!opts.refresh_tokens) {
        return PAM_SUCCESS;
    }

    const char *username = NULL;
    int ret = pam_get_user(pamh, &username, NULL);
    if (ret != PAM_SUCCESS) {
        return PAM_SESSION_ERR;
    }

    auraos_account_t account;
    ret = auraos_account_lookup_by_username(opts.account_db, username, &account);
    if (ret != 0) {
        /* Not an AuraOS user — skip */
        return PAM_SUCCESS;
    }

    /* Retrieve and refresh the token for the new session */
    char refresh_token[2048];
    memset(refresh_token, 0, sizeof(refresh_token));

    ret = auraos_keyring_get_refresh_token(account.sub, refresh_token, sizeof(refresh_token));
    if (ret != 0) {
        auraos_log(LOG_WARNING,
                   "pam_google: session open — could not get refresh token for '%s'",
                   username);
        memset(refresh_token, 0, sizeof(refresh_token));
        return PAM_SUCCESS; /* Non-fatal — user can still log in */
    }

    token_response_t token_resp;
    memset(&token_resp, 0, sizeof(token_resp));

    ret = auraos_refresh_token(refresh_token, &token_resp);
    if (ret == 0) {
        /* Cache the fresh access token in kernel keyring */
        auraos_keyring_cache_access_token(account.sub, token_resp.access_token,
                                           token_resp.expires_in);

        if (token_resp.has_new_refresh_token) {
            auraos_keyring_store_refresh_token(account.sub, token_resp.refresh_token);
        }

        if (opts.debug) {
            auraos_log(LOG_DEBUG,
                       "pam_google: session token refreshed for '%s'", username);
        }
    }

    /* Clear sensitive data */
    memset(&token_resp, 0, sizeof(token_resp));
    memset(refresh_token, 0, sizeof(refresh_token));

    return PAM_SUCCESS;
}

/* ─── pam_sm_close_session ─────────────────────────────────────────── */

PAM_EXTERN int pam_sm_close_session(pam_handle_t *pamh, int flags,
                                     int argc, const char **argv) {
    /* Nothing to clean up — kernel keyring entries expire naturally */
    return PAM_SUCCESS;
}
