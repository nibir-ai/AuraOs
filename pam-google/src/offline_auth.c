/*
 * offline_auth.c — Offline PIN Authentication for AuraOS
 *
 * When no network is available, users can authenticate using a PIN
 * set during initial account setup. The PIN is stored as a bcrypt hash
 * in the account record. This mirrors Chromebook offline login behavior.
 *
 * Copyright (C) 2025 AuraOS Contributors
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

#include <crypt.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <syslog.h>
#include <unistd.h>

#include "pam_google.h"

/* ─── PIN Verification ─────────────────────────────────────────────── */

bool auraos_offline_verify_pin(const char *pin, const char *stored_hash) {
    if (!pin || !stored_hash || strlen(stored_hash) == 0) {
        return false;
    }

    /* Use crypt_r for thread safety */
    struct crypt_data cdata;
    memset(&cdata, 0, sizeof(cdata));

    char *result = crypt_r(pin, stored_hash, &cdata);
    if (!result) {
        auraos_log(LOG_ERR, "pam_google: crypt_r failed during PIN verification");
        memset(&cdata, 0, sizeof(cdata));
        return false;
    }

    bool match = (strcmp(result, stored_hash) == 0);

    /* Clear sensitive data */
    memset(&cdata, 0, sizeof(cdata));

    return match;
}

/* ─── PIN Hashing ──────────────────────────────────────────────────── */

int auraos_offline_hash_pin(const char *pin, char *out_hash, size_t hash_len) {
    if (!pin || !out_hash || hash_len < 64) {
        return -1;
    }

    /* Validate PIN length */
    size_t pin_len = strlen(pin);
    if (pin_len < OFFLINE_PIN_MIN_LENGTH || pin_len > OFFLINE_PIN_MAX_LENGTH) {
        auraos_log(LOG_ERR,
                   "pam_google: PIN length %zu is outside allowed range [%d, %d]",
                   pin_len, OFFLINE_PIN_MIN_LENGTH, OFFLINE_PIN_MAX_LENGTH);
        return -1;
    }

    /*
     * Generate a bcrypt salt with the configured work factor.
     * Format: $2b$<work_factor>$<22 base64 chars>
     *
     * We use /dev/urandom for the salt entropy.
     */
    unsigned char salt_raw[16];
    FILE *urandom = fopen("/dev/urandom", "r");
    if (!urandom) {
        auraos_log(LOG_ERR, "pam_google: cannot open /dev/urandom");
        return -1;
    }

    if (fread(salt_raw, 1, sizeof(salt_raw), urandom) != sizeof(salt_raw)) {
        fclose(urandom);
        auraos_log(LOG_ERR, "pam_google: failed to read from /dev/urandom");
        return -1;
    }
    fclose(urandom);

    /*
     * Use crypt_gensalt to create a proper bcrypt salt string.
     * The $2b$ prefix indicates the bcrypt variant.
     */
    char salt[64];
    snprintf(salt, sizeof(salt), "$2b$%02d$", BCRYPT_WORK_FACTOR);

    /* Encode the raw salt bytes as the bcrypt base64 alphabet */
    static const char b64[] =
        "./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

    size_t prefix_len = strlen(salt);
    for (int i = 0; i < 16 && (prefix_len + i) < sizeof(salt) - 1; i++) {
        salt[prefix_len + i] = b64[salt_raw[i] % 64];
    }
    salt[prefix_len + 16] = '\0';

    /* Clear raw salt */
    memset(salt_raw, 0, sizeof(salt_raw));

    /* Hash the PIN */
    struct crypt_data cdata;
    memset(&cdata, 0, sizeof(cdata));

    char *result = crypt_r(pin, salt, &cdata);
    if (!result) {
        auraos_log(LOG_ERR, "pam_google: crypt_r failed during PIN hashing");
        memset(&cdata, 0, sizeof(cdata));
        memset(salt, 0, sizeof(salt));
        return -1;
    }

    strncpy(out_hash, result, hash_len - 1);
    out_hash[hash_len - 1] = '\0';

    /* Clear sensitive data */
    memset(&cdata, 0, sizeof(cdata));
    memset(salt, 0, sizeof(salt));

    return 0;
}

/* ─── Offline Availability Check ───────────────────────────────────── */

bool auraos_offline_is_available(const auraos_account_t *account) {
    if (!account) {
        return false;
    }

    /* Offline auth is available if an offline PIN hash has been configured */
    return (strlen(account->offline_pin_hash) > 0);
}
