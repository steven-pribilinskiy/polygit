---
title: Installation
description: Build and install polygit from source with cargo and make.
---

`polygit` is a single Rust binary. Build it from source with `cargo`.

## Requirements

- Rust stable (with `cargo`) — install via [rustup](https://rustup.rs)
- `git` on your `PATH`

## Build & install

```bash
git clone https://github.com/steven-pribilinskiy/polygit.git
cd polygit

make build      # release binary at: bin/polygit
make install    # also copies to ~/bin/polygit (+ the bash sibling)
```

`make install` does three things:

1. `cargo build --release`
2. copies the binary to `~/bin/polygit`
3. copies the `polygit-repos` bash backend into `~/bin/polygit-siblings/`

Make sure `~/bin` is on your `PATH`. Verify the install:

```bash
polygit --version
```

## Other make targets

| Target | What it does |
|--------|--------------|
| `make build` | Release build → `bin/polygit` |
| `make install` | Build, then copy to `~/bin` (and the bash sibling) |
| `make test` | `cargo test` |
| `make bench` | Time a `--no-tui` run of the current directory |
| `make clean` | `cargo clean` + remove `bin/polygit` |

## Next steps

- [Usage](../usage/) — run it, pass flags, and read the panes.
- [Keybindings](../../guides/keybindings/) — drive it from the keyboard.
