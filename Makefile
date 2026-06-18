.PHONY: build dev install test bench clean

# Where the runnable `polygit` lives on your $PATH. Override with `make BINDIR=/some/dir`.
BINDIR ?= $(HOME)/bin

# Build the release binary, refresh the repo's bin/, and install it onto $PATH. The install is
# an atomic rename, not a plain cp: copying over a running binary fails with "Text file busy",
# and the rename is what polygit's in-app new-build watcher keys on (the `↺ [reload]` notice).
build:
	cargo build --release
	@cp target/release/polygit bin/polygit
	@echo "→ refreshed repo binary: bin/polygit"
	@mkdir -p $(BINDIR)
	@cp target/release/polygit $(BINDIR)/polygit.new
	@mv -f $(BINDIR)/polygit.new $(BINDIR)/polygit
	@echo "→ installed on \$$PATH (atomic): $(BINDIR)/polygit"
	@echo "✓ build complete"

# Fast inner-loop build: same atomic refresh+install as `build`, but via the `release-fast` profile
# (no whole-program LTO, parallel codegen) — drops most of `build`'s link time for quick iteration.
# Use `make build` for the fully-optimized release that ships.
dev:
	cargo build --profile release-fast
	@cp target/release-fast/polygit bin/polygit
	@echo "→ refreshed repo binary: bin/polygit (release-fast)"
	@mkdir -p $(BINDIR)
	@cp target/release-fast/polygit $(BINDIR)/polygit.new
	@mv -f $(BINDIR)/polygit.new $(BINDIR)/polygit
	@echo "→ installed on \$$PATH (atomic): $(BINDIR)/polygit"
	@echo "✓ dev build complete (release-fast)"

# `build` already builds and installs the binary onto $PATH; `install` is kept as an alias.
install: build

test:
	cargo test

bench:
	@echo "Running benchmark on current directory (use --timeout 5 for quick mode)..."
	time bin/polygit --no-tui 2>&1

clean:
	cargo clean
	rm -f bin/polygit
