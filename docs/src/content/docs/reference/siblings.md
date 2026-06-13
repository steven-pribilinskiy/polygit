---
title: Sibling builds
description: How polygit forwards to the Go, Bun, and bash implementations via subcommands.
---

`polygit` is the canonical Rust build, but it also fronts three sibling implementations.
When the first argument is `go`, `bun`, or `cli`, it replaces its own process with the
matching backend and forwards every remaining argument verbatim.

```bash
polygit go  [args]   # Go / bubbletea build      (polygit-tui-go)
polygit bun [args]   # Bun / ink build, JIT       (polygit-tui-bun-jit)
polygit cli [args]   # bash streaming version     (polygit-repos)
```

A directory literally named `go`/`bun`/`cli` is still reachable as `polygit ./go`, etc.

## Where the backends live

The backends live in a `polygit-siblings/` directory next to the `polygit` binary
(e.g. `~/bin/polygit-siblings/`), deliberately **off `$PATH`** so they aren't top-level
commands — they're reachable only through `polygit go|bun|cli`. The dispatcher resolves
them relative to its own location, falling back to the bare name on `$PATH` if that
directory is absent.

The `cli` backend (`polygit-repos`, the original parallel-pull bash script that the
plain-mode output was ported from) is tracked in the repo and deployed by `make install`.
The `go` and `bun` backends are built from their own source trees.
