// config.rs — AuraOS OAuth Configuration
//
// Compile-time embedded OAuth client credentials and scope definitions.
// The client ID is public; the client secret is obfuscated per Google's
// installed-app OAuth documentation (security comes from PKCE, not the secret).
//
// Copyright (C) 2025 AuraOS Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/// AuraOS registered OAuth 2.0 Client ID
/// This is public and baked into the distributed binary.
/// Replace with actual registered client ID before building.
pub const CLIENT_ID: &str = "AURAOS_CLIENT_ID_PLACEHOLDER.apps.googleusercontent.com";

/// AuraOS registered OAuth 2.0 Client Secret
/// For installed (desktop) apps, Google documents that this is "not secret."
/// Security comes from PKCE, not the client secret.
/// Replace with actual registered client secret before building.
pub const CLIENT_SECRET: &str = "AURAOS_CLIENT_SECRET_PLACEHOLDER";

/// OAuth scopes requested during sign-in
/// These cover all Google services AuraOS integrates with.
pub const OAUTH_SCOPES: &[&str] = &[
    // OpenID Connect
    "openid",
    "email",
    "profile",

    // Gmail — full access for search, send, draft
    "https://mail.google.com/",

    // Google Calendar — read/write events
    "https://www.googleapis.com/auth/calendar",

    // Google Drive — read-only (FUSE mount, search)
    "https://www.googleapis.com/auth/drive.readonly",

    // Google Contacts — read-only (for Gemini contact search)
    "https://www.googleapis.com/auth/contacts.readonly",

    // Google Tasks — read/write
    "https://www.googleapis.com/auth/tasks",

    // User profile info
    "https://www.googleapis.com/auth/userinfo.profile",
];

/// Default aura-cloud backend URL
pub const AURA_CLOUD_URL: &str = "https://api.auraos.io:443";

/// Configuration file path
pub const CONFIG_PATH: &str = "/etc/auraos/config.toml";

/// Account database path
pub const DEFAULT_ACCOUNT_DB: &str = "/var/lib/auraos/accounts";
