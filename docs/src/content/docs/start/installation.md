---
title: Installation
description: Build and install pull-all from source with cargo and make.
---

`pull-all` is a single Rust binary. Build it from source with `cargo`.

## Requirements

- Rust stable (with `cargo`) — install via [rustup](https://rustup.rs)
- `git` on your `PATH`

## Build & install

```bash
git clone https://github.com/steven-pribilinskiy/pull-all.git
cd pull-all

make build      # release binary at: bin/pull-all
make install    # also copies to ~/bin/pull-all (+ the bash sibling)
```

`make install` does three things:

1. `cargo build --release`
2. copies the binary to `~/bin/pull-all`
3. copies the `pull-all-repos` bash backend into `~/bin/pull-all-siblings/`

Make sure `~/bin` is on your `PATH`. Verify the install:

```bash
pull-all --version
```

## Other make targets

| Target | What it does |
|--------|--------------|
| `make build` | Release build → `bin/pull-all` |
| `make install` | Build, then copy to `~/bin` (and the bash sibling) |
| `make test` | `cargo test` |
| `make bench` | Time a `--no-tui` run of the current directory |
| `make clean` | `cargo clean` + remove `bin/pull-all` |

## Next steps

- [Usage](../usage/) — run it, pass flags, and read the panes.
- [Keybindings](../../guides/keybindings/) — drive it from the keyboard.
