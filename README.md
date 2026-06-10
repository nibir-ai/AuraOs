# AuraOS

**A Google-Integrated Linux Distribution with Gemini Personal Assistant**

AuraOS is a purpose-built Linux distribution designed to replicate the cohesive, identity-centric experience of Android and ChromeOS on the general-purpose desktop. Your Google account is your operating system identity, and Gemini is a first-class system service.

## Architecture

```
┌──────────────┐   ┌──────────────┐   ┌──────────────┐   ┌────────────┐
│ GNOME Shell  │   │  Aura Panel  │   │  GTK4 Apps   │   │  CLI Tool  │
│  Extension   │   │  (Indicator) │   │ (via libgem) │   │ (aura-cli) │
└──────┬───────┘   └──────┬───────┘   └──────┬───────┘   └─────┬──────┘
       └──────────────────┴──────────────────┴──────────────────┘
                                   │
                             D-Bus (Session Bus)
                    com.auraos.GeminiAssistant interface
                                   │
                   ┌───────────────▼──────────────────┐
                   │           gemini-daemon            │
                   │  (Rust, async tokio runtime)       │
                   │                                    │
                   │  ┌─────────────────────────────┐  │
                   │  │   Tool Orchestrator (MCP)    │  │
                   │  │   Gmail │ Calendar │ Drive   │  │
                   │  └─────────────────────────────┘  │
                   └────────────────────────────────────┘
                                   │
                           TLS 1.3 / gRPC
                                   │
                   ┌───────────────▼──────────────────┐
                   │        aura-cloud backend          │
                   │    (GCP Cloud Run — holds API key) │
                   └────────────────────────────────────┘
                                   │
                   ┌───────────────▼──────────────────┐
                   │    Google Gemini API               │
                   └───────────────────────────────────┘
```

## Components

| Component | Language | Description |
|---|---|---|
| `pam-google` | C | PAM authentication module — login with Google |
| `aura-auth-helper` | Rust | OAuth 2.0 PKCE flow for first-boot sign-in |
| `aura-token-refresh` | Rust | systemd service for automatic token refresh |
| `aura-profile-sync` | Rust | Google profile sync (name, avatar, contacts) |
| `gemini-daemon` | Rust | Core AI system service with D-Bus interface |
| `aura-cli` | Rust | Command-line interface to Gemini |
| `aura-cloud` | Go | Backend proxy holding Gemini API key |
| `gnome-shell-extension` | TypeScript | Desktop Gemini panel + account switcher |

## Base Distribution

- **Base:** Ubuntu 24.04 LTS (Noble Numbat)
- **Desktop:** GNOME 46 (modified)
- **Kernel:** Ubuntu HWE 6.8+
- **Display:** Wayland-first (Mutter) + XWayland
- **Init:** systemd 255+
- **Security:** AppArmor, LUKS2 + TPM2

## Building

### Prerequisites

- Ubuntu 24.04 or equivalent (for native builds)
- Rust 1.75+ (via rustup)
- Go 1.22+
- GCC, CMake, libpam-dev, libsecret-1-dev, libcurl4-openssl-dev, libjson-c-dev
- protobuf-compiler, protoc-gen-go, protoc-gen-go-grpc
- Node.js 20+ (for GNOME Shell extension TypeScript compilation)

### Build All

```bash
make all
```

### Build Individual Components

```bash
make pam-google          # PAM module
make aura-auth-helper    # OAuth helper
make aura-token-refresh  # Token refresh service
make aura-profile-sync   # Profile sync service
make gemini-daemon       # Gemini system daemon
make aura-cli            # CLI tool
make aura-cloud          # Backend service
make gnome-extension     # GNOME Shell extension
```

### Build Debian Packages

```bash
make deb
```

### Build ISO

```bash
make iso
```

## Project Structure

```
AuraOs/
├── aura-auth-helper/      Rust — OAuth PKCE flow
├── aura-cli/              Rust — CLI interface
├── aura-cloud/            Go — Backend proxy
├── aura-profile-sync/     Rust — Profile sync
├── aura-token-refresh/    Rust — Token refresh
├── gemini-daemon/         Rust — Core AI service
├── gnome-shell-extension/ TypeScript — Desktop integration
├── pam-google/            C — PAM module
├── apparmor/              AppArmor profiles
├── config/                Default configuration
├── dbus/                  D-Bus interface definitions
├── installer/             Calamares customization
├── iso-build/             ISO build scripts
├── packaging/             Debian packaging
├── proto/                 Shared protobuf definitions
└── systemd/               systemd unit files
```

## License

GNU General Public License v3.0 — See [LICENSE](LICENSE)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.
