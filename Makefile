BIN        := odin
PREFIX     ?= $(HOME)/.local
BINDIR     := $(PREFIX)/bin
DEBUG_BIN  := target/debug/$(BIN)
RELEASE_BIN := target/release/$(BIN)
ODIN_VERSION := $(shell grep -m1 '^version = ' Cargo.toml | sed -E 's/^version = "(.*)"$$/\1/')

.PHONY: all build release install install-user uninstall uninstall-user deb rpm run test lint fmt fmt-check check clean help web-install web-build web-dev

all: build

## Install the dashboard frontend's npm dependencies
web-install:
	npm --prefix web ci

## Build the dashboard frontend (output embedded into the binary from web/dist)
web-build: web-install
	VITE_ODIN_VERSION=$(ODIN_VERSION) npm --prefix web run build

## Run the dashboard frontend's Vite dev server (proxies /api to `odin serve`)
web-dev:
	npm --prefix web run dev

## Build a debug binary (target/debug/odin), including the dashboard frontend
build: web-build
	cargo build

## Build an optimized release binary (target/release/odin), including the dashboard frontend
release: web-build
	cargo build --release

## Build a .deb package (needs `cargo install cargo-deb`)
deb: web-build
	cargo deb

## Build an .rpm package (needs `cargo install cargo-generate-rpm`)
rpm: release
	cargo generate-rpm

## Build and install a system-wide .deb/.rpm package (needs sudo; Debian/Fedora-family only)
install:
	@sudo -v
	@if [ -f /etc/debian_version ]; then \
		$(MAKE) deb; \
		sudo apt install --reinstall -y ./target/debian/odin_*.deb; \
	elif [ -f /etc/redhat-release ] || [ -f /etc/fedora-release ]; then \
		$(MAKE) rpm; \
		sudo dnf reinstall -y ./target/generate-rpm/odin-*.rpm || \
			sudo dnf install -y ./target/generate-rpm/odin-*.rpm; \
	else \
		echo "Unsupported distro (no /etc/debian_version or /etc/redhat-release)." >&2; \
		echo "Use 'make install-user' for a per-user install, or 'make deb'/'make rpm'" >&2; \
		echo "to build a package manually." >&2; \
		exit 1; \
	fi

## Install the release binary to $(BINDIR) instead of a system package (override with `make install-user PREFIX=/usr/local`)
install-user: release
	install -Dm755 $(RELEASE_BIN) $(BINDIR)/$(BIN)
	@echo "Installed to $(BINDIR)/$(BIN)"
	@echo "Make sure $(BINDIR) is on your PATH."

## Remove the system-wide .deb/.rpm package (needs sudo)
uninstall:
	@sudo -v
	@if [ -f /etc/debian_version ]; then \
		sudo apt remove -y odin; \
	elif command -v dnf >/dev/null 2>&1; then \
		sudo dnf remove -y odin; \
	else \
		echo "Not a package-managed install; use 'make uninstall-user' instead." >&2; \
		exit 1; \
	fi

## Remove the binary installed by `make install-user`
uninstall-user:
	rm -f $(BINDIR)/$(BIN)

## Run the debug binary, forwarding extra args: make run ARGS="status"
run:
	cargo run -- $(ARGS)

## Run the test suite
test:
	cargo test

## Run clippy across all targets, denying warnings
lint:
	cargo clippy --all-targets -- -D warnings

## Format the codebase
fmt:
	cargo fmt

## Check formatting without modifying files
fmt-check:
	cargo fmt --check

## Run fmt-check, lint, and test together (use before committing)
check: fmt-check lint test

## Remove build artifacts
clean:
	cargo clean

## List available targets
help:
	@grep -E '^## ' -A1 $(MAKEFILE_LIST) | grep -v '^--' | \
		awk '/^## /{desc=substr($$0,4)} /^[a-zA-Z_-]+:/{split($$0,a,":"); printf "  %-12s %s\n", a[1], desc}'
