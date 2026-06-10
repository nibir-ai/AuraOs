/*
 * token_refresh.c — OAuth 2.0 Token Refresh for AuraOS PAM
 *
 * Handles refreshing Google OAuth access tokens using the stored refresh
 * token, and validating access tokens against Google's tokeninfo endpoint.
 *
 * Copyright (C) 2025 AuraOS Contributors
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

#include <curl/curl.h>
#include <json-c/json.h>
#include <string.h>
#include <stdlib.h>
#include <syslog.h>

#include "pam_google.h"

/* ─── cURL Write Callback ──────────────────────────────────────────── */

typedef struct {
    char  *data;
    size_t size;
    size_t capacity;
} response_buffer_t;

static size_t write_callback(void *contents, size_t size, size_t nmemb,
                              void *userp) {
    size_t total = size * nmemb;
    response_buffer_t *buf = (response_buffer_t *)userp;

    if (buf->size + total >= buf->capacity) {
        size_t new_cap = buf->capacity * 2;
        if (new_cap < buf->size + total + 1) {
            new_cap = buf->size + total + 1;
        }
        char *new_data = realloc(buf->data, new_cap);
        if (!new_data) {
            return 0; /* Signal error to cURL */
        }
        buf->data = new_data;
        buf->capacity = new_cap;
    }

    memcpy(buf->data + buf->size, contents, total);
    buf->size += total;
    buf->data[buf->size] = '\0';

    return total;
}

static void response_buffer_init(response_buffer_t *buf) {
    buf->capacity = 4096;
    buf->data = malloc(buf->capacity);
    buf->size = 0;
    if (buf->data) {
        buf->data[0] = '\0';
    }
}

static void response_buffer_free(response_buffer_t *buf) {
    if (buf->data) {
        /* Zero out before freeing — may contain tokens */
        memset(buf->data, 0, buf->capacity);
        free(buf->data);
        buf->data = NULL;
    }
    buf->size = 0;
    buf->capacity = 0;
}

/* ─── Token Refresh ────────────────────────────────────────────────── */

int auraos_refresh_token(const char *refresh_token, token_response_t *response) {
    if (!refresh_token || !response) {
        return -1;
    }

    CURL *curl = curl_easy_init();
    if (!curl) {
        auraos_log(LOG_ERR, "pam_google: failed to init cURL");
        return -1;
    }

    /* Build POST body */
    char post_fields[4096];
    snprintf(post_fields, sizeof(post_fields),
             "client_id=%s"
             "&client_secret=%s"
             "&refresh_token=%s"
             "&grant_type=refresh_token",
             AURAOS_CLIENT_ID,
             AURAOS_CLIENT_SECRET,
             refresh_token);

    response_buffer_t resp_buf;
    response_buffer_init(&resp_buf);

    if (!resp_buf.data) {
        curl_easy_cleanup(curl);
        return -1;
    }

    curl_easy_setopt(curl, CURLOPT_URL, GOOGLE_TOKEN_ENDPOINT);
    curl_easy_setopt(curl, CURLOPT_POSTFIELDS, post_fields);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, write_callback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &resp_buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT_MS, (long)TOKEN_REFRESH_TIMEOUT_MS);
    curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT, 5L);
    curl_easy_setopt(curl, CURLOPT_SSL_VERIFYPEER, 1L);
    curl_easy_setopt(curl, CURLOPT_SSL_VERIFYHOST, 2L);
    /* Disable following redirects for security */
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 0L);

    CURLcode res = curl_easy_perform(curl);

    long http_code = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &http_code);

    /* Clear post fields containing secrets */
    memset(post_fields, 0, sizeof(post_fields));

    if (res != CURLE_OK) {
        auraos_log(LOG_ERR, "pam_google: token refresh network error: %s",
                   curl_easy_strerror(res));
        curl_easy_cleanup(curl);
        response_buffer_free(&resp_buf);
        return -1;
    }

    curl_easy_cleanup(curl);

    if (http_code == 400 || http_code == 401) {
        /* Token revoked or invalid */
        auraos_log(LOG_WARNING,
                   "pam_google: token refresh returned HTTP %ld — token likely revoked",
                   http_code);
        response_buffer_free(&resp_buf);
        return -2;
    }

    if (http_code != 200) {
        auraos_log(LOG_ERR,
                   "pam_google: token refresh unexpected HTTP %ld", http_code);
        response_buffer_free(&resp_buf);
        return -1;
    }

    /* Parse JSON response */
    struct json_object *root = json_tokener_parse(resp_buf.data);
    if (!root) {
        auraos_log(LOG_ERR, "pam_google: failed to parse token refresh JSON");
        response_buffer_free(&resp_buf);
        return -1;
    }

    struct json_object *obj;

    if (json_object_object_get_ex(root, "access_token", &obj)) {
        strncpy(response->access_token, json_object_get_string(obj),
                sizeof(response->access_token) - 1);
    }

    if (json_object_object_get_ex(root, "refresh_token", &obj)) {
        strncpy(response->refresh_token, json_object_get_string(obj),
                sizeof(response->refresh_token) - 1);
        response->has_new_refresh_token = true;
    } else {
        response->has_new_refresh_token = false;
    }

    if (json_object_object_get_ex(root, "id_token", &obj)) {
        strncpy(response->id_token, json_object_get_string(obj),
                sizeof(response->id_token) - 1);
    }

    if (json_object_object_get_ex(root, "expires_in", &obj)) {
        response->expires_in = json_object_get_int(obj);
    }

    if (json_object_object_get_ex(root, "scope", &obj)) {
        strncpy(response->scope, json_object_get_string(obj),
                sizeof(response->scope) - 1);
    }

    json_object_put(root);
    response_buffer_free(&resp_buf);

    return 0;
}

/* ─── Token Validation ─────────────────────────────────────────────── */

int auraos_validate_token(const char *access_token, char *out_sub, size_t sub_len) {
    if (!access_token || !out_sub || sub_len == 0) {
        return -1;
    }

    CURL *curl = curl_easy_init();
    if (!curl) {
        return -1;
    }

    /* Build tokeninfo URL */
    char url[2048];
    char *encoded_token = curl_easy_escape(curl, access_token, 0);
    if (!encoded_token) {
        curl_easy_cleanup(curl);
        return -1;
    }
    snprintf(url, sizeof(url), "%s?access_token=%s",
             GOOGLE_TOKENINFO_ENDPOINT, encoded_token);
    curl_free(encoded_token);

    response_buffer_t resp_buf;
    response_buffer_init(&resp_buf);

    if (!resp_buf.data) {
        curl_easy_cleanup(curl);
        return -1;
    }

    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_HTTPGET, 1L);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, write_callback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &resp_buf);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT_MS, (long)TOKEN_REFRESH_TIMEOUT_MS);
    curl_easy_setopt(curl, CURLOPT_SSL_VERIFYPEER, 1L);
    curl_easy_setopt(curl, CURLOPT_SSL_VERIFYHOST, 2L);

    CURLcode res = curl_easy_perform(curl);
    long http_code = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &http_code);
    curl_easy_cleanup(curl);

    if (res != CURLE_OK || http_code != 200) {
        response_buffer_free(&resp_buf);
        return -1;
    }

    /* Parse response for "sub" field */
    struct json_object *root = json_tokener_parse(resp_buf.data);
    if (!root) {
        response_buffer_free(&resp_buf);
        return -1;
    }

    struct json_object *sub_obj;
    if (json_object_object_get_ex(root, "sub", &sub_obj)) {
        strncpy(out_sub, json_object_get_string(sub_obj), sub_len - 1);
        out_sub[sub_len - 1] = '\0';
    } else {
        json_object_put(root);
        response_buffer_free(&resp_buf);
        return -1;
    }

    json_object_put(root);
    response_buffer_free(&resp_buf);

    return 0;
}
