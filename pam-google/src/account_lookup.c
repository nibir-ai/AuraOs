/*
 * account_lookup.c — AuraOS Account Database Operations
 *
 * Manages reading and writing AuraOS account records stored as JSON files
 * in /var/lib/auraos/accounts/<sub>.json. Each file contains metadata
 * about a Google account mapped to a Linux user (no tokens stored here).
 *
 * Copyright (C) 2025 AuraOS Contributors
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

#include <json-c/json.h>
#include <dirent.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <sys/stat.h>
#include <errno.h>
#include <syslog.h>

#include "pam_google.h"

/* ─── JSON Helpers ─────────────────────────────────────────────────── */

static void json_get_string(struct json_object *root, const char *key,
                             char *out, size_t out_len) {
    struct json_object *obj;
    if (json_object_object_get_ex(root, key, &obj)) {
        const char *val = json_object_get_string(obj);
        if (val) {
            strncpy(out, val, out_len - 1);
            out[out_len - 1] = '\0';
        }
    }
}

static int parse_account_json(const char *json_str, auraos_account_t *account) {
    struct json_object *root = json_tokener_parse(json_str);
    if (!root) {
        return -1;
    }

    json_get_string(root, "sub", account->sub, sizeof(account->sub));
    json_get_string(root, "email", account->email, sizeof(account->email));
    json_get_string(root, "display_name", account->display_name, sizeof(account->display_name));
    json_get_string(root, "avatar_url", account->avatar_url, sizeof(account->avatar_url));
    json_get_string(root, "linux_username", account->linux_username, sizeof(account->linux_username));
    json_get_string(root, "scopes_granted", account->scopes_granted, sizeof(account->scopes_granted));
    json_get_string(root, "account_created", account->account_created, sizeof(account->account_created));
    json_get_string(root, "offline_pin_hash", account->offline_pin_hash, sizeof(account->offline_pin_hash));

    json_object_put(root);
    return 0;
}

static int read_file_contents(const char *path, char **out, size_t *out_len) {
    FILE *f = fopen(path, "r");
    if (!f) {
        return -1;
    }

    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (len <= 0 || len > 1048576) { /* 1MB max */
        fclose(f);
        return -1;
    }

    *out = malloc((size_t)len + 1);
    if (!*out) {
        fclose(f);
        return -1;
    }

    size_t read = fread(*out, 1, (size_t)len, f);
    fclose(f);

    (*out)[read] = '\0';
    if (out_len) {
        *out_len = read;
    }

    return 0;
}

/* ─── Account Lookup by Sub ────────────────────────────────────────── */

int auraos_account_lookup_by_sub(const char *account_db_path,
                                  const char *sub,
                                  auraos_account_t *account) {
    if (!account_db_path || !sub || !account) {
        return -1;
    }

    memset(account, 0, sizeof(*account));

    /* Build path: /var/lib/auraos/accounts/<sub>.json */
    char path[1024];
    snprintf(path, sizeof(path), "%s/%s.json", account_db_path, sub);

    /* Security: verify the sub doesn't contain path traversal characters */
    if (strchr(sub, '/') || strchr(sub, '\\') || strstr(sub, "..")) {
        auraos_log(LOG_ERR, "pam_google: invalid sub (path traversal attempt): %s", sub);
        return -1;
    }

    char *contents = NULL;
    size_t contents_len = 0;

    if (read_file_contents(path, &contents, &contents_len) != 0) {
        return -1;
    }

    int ret = parse_account_json(contents, account);

    /* Clear and free file contents */
    memset(contents, 0, contents_len);
    free(contents);

    return ret;
}

/* ─── Account Lookup by Username ───────────────────────────────────── */

int auraos_account_lookup_by_username(const char *account_db_path,
                                       const char *username,
                                       auraos_account_t *account) {
    if (!account_db_path || !username || !account) {
        return -1;
    }

    memset(account, 0, sizeof(*account));

    /* Scan all account JSON files and find matching linux_username */
    DIR *dir = opendir(account_db_path);
    if (!dir) {
        auraos_log(LOG_ERR, "pam_google: cannot open account db '%s': %s",
                   account_db_path, strerror(errno));
        return -1;
    }

    struct dirent *entry;
    int found = -1;

    while ((entry = readdir(dir)) != NULL) {
        /* Only process .json files */
        size_t name_len = strlen(entry->d_name);
        if (name_len < 6 || strcmp(entry->d_name + name_len - 5, ".json") != 0) {
            continue;
        }

        char path[1024];
        snprintf(path, sizeof(path), "%s/%s", account_db_path, entry->d_name);

        char *contents = NULL;
        size_t contents_len = 0;

        if (read_file_contents(path, &contents, &contents_len) != 0) {
            continue;
        }

        auraos_account_t tmp;
        memset(&tmp, 0, sizeof(tmp));

        if (parse_account_json(contents, &tmp) == 0) {
            if (strcmp(tmp.linux_username, username) == 0) {
                memcpy(account, &tmp, sizeof(*account));
                found = 0;
            }
        }

        memset(contents, 0, contents_len);
        free(contents);

        if (found == 0) {
            break;
        }
    }

    closedir(dir);
    return found;
}

/* ─── Account Save ─────────────────────────────────────────────────── */

int auraos_account_save(const char *account_db_path,
                         const auraos_account_t *account) {
    if (!account_db_path || !account || strlen(account->sub) == 0) {
        return -1;
    }

    /* Security: verify sub doesn't contain path traversal */
    if (strchr(account->sub, '/') || strchr(account->sub, '\\') ||
        strstr(account->sub, "..")) {
        auraos_log(LOG_ERR, "pam_google: invalid sub for save: %s", account->sub);
        return -1;
    }

    /* Build JSON object */
    struct json_object *root = json_object_new_object();
    if (!root) {
        return -1;
    }

    json_object_object_add(root, "sub",
                           json_object_new_string(account->sub));
    json_object_object_add(root, "email",
                           json_object_new_string(account->email));
    json_object_object_add(root, "display_name",
                           json_object_new_string(account->display_name));
    json_object_object_add(root, "avatar_url",
                           json_object_new_string(account->avatar_url));
    json_object_object_add(root, "linux_username",
                           json_object_new_string(account->linux_username));
    json_object_object_add(root, "scopes_granted",
                           json_object_new_string(account->scopes_granted));
    json_object_object_add(root, "account_created",
                           json_object_new_string(account->account_created));

    /* Only write offline_pin_hash if set */
    if (strlen(account->offline_pin_hash) > 0) {
        json_object_object_add(root, "offline_pin_hash",
                               json_object_new_string(account->offline_pin_hash));
    }

    const char *json_str = json_object_to_json_string_ext(root,
                                                           JSON_C_TO_STRING_PRETTY);

    /* Write to file atomically (write to .tmp, then rename) */
    char path[1024];
    char tmp_path[1024];
    snprintf(path, sizeof(path), "%s/%s.json", account_db_path, account->sub);
    snprintf(tmp_path, sizeof(tmp_path), "%s/%s.json.tmp", account_db_path, account->sub);

    FILE *f = fopen(tmp_path, "w");
    if (!f) {
        auraos_log(LOG_ERR, "pam_google: cannot write account file '%s': %s",
                   tmp_path, strerror(errno));
        json_object_put(root);
        return -1;
    }

    fprintf(f, "%s\n", json_str);
    fclose(f);

    /* Set file permissions to 600 (owner-only read/write) */
    chmod(tmp_path, 0600);

    /* Atomic rename */
    if (rename(tmp_path, path) != 0) {
        auraos_log(LOG_ERR, "pam_google: failed to rename '%s' to '%s': %s",
                   tmp_path, path, strerror(errno));
        unlink(tmp_path);
        json_object_put(root);
        return -1;
    }

    json_object_put(root);
    return 0;
}
