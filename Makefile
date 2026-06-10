# AuraOS — Top-Level Build Orchestration
# Requires: Rust (cargo), Go, GCC, CMake, protoc, Node.js

.PHONY: all clean pam-google aura-auth-helper aura-token-refresh aura-profile-sync \
        gemini-daemon aura-cli aura-installer aura-cloud gnome-extension proto deb iso

# Build output directory
BUILD_DIR := build
PREFIX := /usr

# ─── Proto Compilation ──────────────────────────────────────────────
proto:
	@echo "=== Compiling protobuf definitions ==="
	mkdir -p aura-cloud/proto/auracloudv1
	protoc --go_out=aura-cloud --go-grpc_out=aura-cloud \
		--go_opt=module=github.com/auraos/aura-cloud \
		--go-grpc_opt=module=github.com/auraos/aura-cloud \
		--proto_path=proto proto/aura_cloud.proto

# ─── C Components ───────────────────────────────────────────────────
pam-google:
	@echo "=== Building pam-google ==="
	mkdir -p pam-google/build
	cd pam-google/build && cmake .. -DCMAKE_BUILD_TYPE=Release && make -j$$(nproc)

# ─── Rust Components ───────────────────────────────────────────────
aura-auth-helper:
	@echo "=== Building aura-auth-helper ==="
	cd aura-auth-helper && cargo build --release
	mkdir -p $(BUILD_DIR)
	cp aura-auth-helper/target/release/aura-auth-helper $(BUILD_DIR)/aura-auth-helper

aura-token-refresh:
	@echo "=== Building aura-token-refresh ==="
	cd aura-token-refresh && cargo build --release
	mkdir -p $(BUILD_DIR)
	cp aura-token-refresh/target/release/aura-token-refresh $(BUILD_DIR)/aura-token-refresh

aura-profile-sync:
	@echo "=== Building aura-profile-sync ==="
	cd aura-profile-sync && cargo build --release
	mkdir -p $(BUILD_DIR)
	cp aura-profile-sync/target/release/aura-profile-sync $(BUILD_DIR)/aura-profile-sync

gemini-daemon:
	@echo "=== Building gemini-daemon ==="
	cd gemini-daemon && cargo build --release
	mkdir -p $(BUILD_DIR)
	cp gemini-daemon/target/release/gemini-daemon $(BUILD_DIR)/gemini-daemon

aura-cli:
	@echo "=== Building aura-cli ==="
	cd aura-cli && cargo build --release
	mkdir -p $(BUILD_DIR)
	cp aura-cli/target/release/aura-cli $(BUILD_DIR)/aura-cli

aura-installer:
	@echo "=== Building aura-installer ==="
	cd aura-installer && cargo build --release
	mkdir -p $(BUILD_DIR)
	cp aura-installer/target/release/aura-installer $(BUILD_DIR)/aura-installer

# ─── Go Components ─────────────────────────────────────────────────
aura-cloud: proto
	@echo "=== Building aura-cloud ==="
	cd aura-cloud && go build -o ../$(BUILD_DIR)/aura-cloud ./cmd/server

# ─── GNOME Shell Extension ─────────────────────────────────────────
gnome-extension:
	@echo "=== Building GNOME Shell extension ==="
	cd gnome-shell-extension && npx tsc

# ─── Aggregate Targets ─────────────────────────────────────────────
rust-all: aura-auth-helper aura-token-refresh aura-profile-sync gemini-daemon aura-cli aura-installer

all: proto pam-google rust-all aura-cloud gnome-extension
	@echo "=== All components built successfully ==="

# ─── Debian Packages ───────────────────────────────────────────────
deb: all
	@echo "=== Building Debian packages ==="
	cd packaging/gemini-daemon && dpkg-buildpackage -us -uc -b
	cd packaging/aura-auth-helper && dpkg-buildpackage -us -uc -b
	cd packaging/pam-google && dpkg-buildpackage -us -uc -b

# ─── ISO Build ──────────────────────────────────────────────────────
iso: deb
	@echo "=== Building AuraOS ISO ==="
	cd iso-build && sudo bash build-iso.sh

# ─── Install (for development) ─────────────────────────────────────
install: all
	@echo "=== Installing AuraOS components ==="
	# PAM module
	install -Dm644 pam-google/build/pam_google.so $(DESTDIR)$(PREFIX)/lib/security/pam_google.so
	# Rust binaries
	install -Dm755 aura-auth-helper/target/release/aura-auth-helper $(DESTDIR)$(PREFIX)/lib/auraos/aura-auth-helper
	install -Dm755 aura-token-refresh/target/release/aura-token-refresh $(DESTDIR)$(PREFIX)/lib/auraos/aura-token-refresh
	install -Dm755 aura-profile-sync/target/release/aura-profile-sync $(DESTDIR)$(PREFIX)/lib/auraos/aura-profile-sync
	install -Dm755 gemini-daemon/target/release/gemini-daemon $(DESTDIR)$(PREFIX)/libexec/gemini-daemon
	install -Dm755 aura-cli/target/release/aura-cli $(DESTDIR)$(PREFIX)/bin/aura-cli
	install -Dm755 aura-installer/target/release/aura-installer $(DESTDIR)$(PREFIX)/bin/aura-installer
	# D-Bus
	install -Dm644 dbus/com.auraos.GeminiAssistant.xml $(DESTDIR)$(PREFIX)/share/dbus-1/interfaces/com.auraos.GeminiAssistant.xml
	install -Dm644 dbus/com.auraos.GeminiAssistant.service $(DESTDIR)$(PREFIX)/share/dbus-1/services/com.auraos.GeminiAssistant.service
	# systemd units
	install -Dm644 systemd/gemini-daemon.service $(DESTDIR)/etc/systemd/user/gemini-daemon.service
	install -Dm644 systemd/aura-oobe.service $(DESTDIR)/etc/systemd/system/aura-oobe.service
	install -Dm644 systemd/aura-token-refresh@.service $(DESTDIR)/etc/systemd/user/aura-token-refresh@.service
	install -Dm644 systemd/aura-token-refresh@.timer $(DESTDIR)/etc/systemd/user/aura-token-refresh@.timer
	install -Dm644 systemd/aura-profile-sync@.service $(DESTDIR)/etc/systemd/user/aura-profile-sync@.service
	install -Dm644 systemd/aura-profile-sync@.timer $(DESTDIR)/etc/systemd/user/aura-profile-sync@.timer
	install -Dm644 systemd/aura-cloud-sync@.service $(DESTDIR)/etc/systemd/user/aura-cloud-sync@.service
	# Config
	install -Dm644 config/auraos-config.toml $(DESTDIR)/etc/auraos/config.toml
	install -Dm644 config/tool-allowlist.json $(DESTDIR)/etc/auraos/tool-allowlist.json
	install -Dm644 config/pam.d/aura-gdm-password $(DESTDIR)/etc/pam.d/aura-gdm-password
	install -Dm644 config/pam.d/aura-login $(DESTDIR)/etc/pam.d/aura-login
	# AppArmor
	install -Dm644 apparmor/usr.libexec.gemini-daemon $(DESTDIR)/etc/apparmor.d/usr.libexec.gemini-daemon
	install -Dm644 apparmor/usr.lib.auraos.aura-auth-helper $(DESTDIR)/etc/apparmor.d/usr.lib.auraos.aura-auth-helper
	# Create data directories
	install -dm700 $(DESTDIR)/var/lib/auraos/accounts

# ─── Clean ──────────────────────────────────────────────────────────
clean:
	rm -rf $(BUILD_DIR)
	rm -rf pam-google/build
	rm -rf aura-cloud/proto/auracloudv1
	cd aura-auth-helper && cargo clean
	cd aura-token-refresh && cargo clean
	cd aura-profile-sync && cargo clean
	cd gemini-daemon && cargo clean
	cd aura-cli && cargo clean
	cd aura-installer && cargo clean
	cd aura-cloud && go clean
