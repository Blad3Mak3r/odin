BIN        := odin
PREFIX     ?= $(HOME)/.local
BINDIR     := $(PREFIX)/bin
DEBUG_BIN  := target/debug/$(BIN)
RELEASE_BIN := target/release/$(BIN)

.PHONY: all build release install uninstall run test lint fmt fmt-check check clean help

all: build

## Build a debug binary (target/debug/odin)
build:
	cargo build

## Build an optimized release binary (target/release/odin)
release:
	cargo build --release

## Install the release binary to $(BINDIR) (override with `make install PREFIX=/usr/local`)
install: release
	install -Dm755 $(RELEASE_BIN) $(BINDIR)/$(BIN)
	@echo "Installed to $(BINDIR)/$(BIN)"
	@echo "Make sure $(BINDIR) is on your PATH."

## Remove the installed binary from $(BINDIR)
uninstall:
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
