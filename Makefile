.PHONY: build install test bench clean

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
	@echo "✓ build complete — pull/p aliases now run the new build"

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
