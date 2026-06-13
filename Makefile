.PHONY: build install test bench clean

# Where the runnable `polygit` lives on your $PATH. Override with `make BINDIR=/some/dir`.
BINDIR ?= $(HOME)/bin

# Build the release binary, refresh the repo's bin/, and install it onto $PATH. The install is
# an atomic rename, not a plain cp: copying over a running binary fails with "Text file busy",
# and the rename is what polygit's in-app new-build watcher keys on (the `↺ [reload]` notice).
build:
	cargo build --release
	cp target/release/polygit bin/polygit
	@mkdir -p $(BINDIR)
	cp target/release/polygit $(BINDIR)/polygit.new
	mv -f $(BINDIR)/polygit.new $(BINDIR)/polygit

# `build` already installs the main binary; this adds the sibling backends (go/bun/bash).
install: build
	mkdir -p $(BINDIR)/polygit-siblings
	cp polygit-siblings/polygit-repos $(BINDIR)/polygit-siblings/polygit-repos

test:
	cargo test

bench:
	@echo "Running benchmark on current directory (use --timeout 5 for quick mode)..."
	time bin/polygit --no-tui 2>&1

clean:
	cargo clean
	rm -f bin/polygit
