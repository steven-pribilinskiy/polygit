---
title: Sibling builds
description: How pull-all forwards to the Go, Bun, and bash implementations via subcommands.
---

`pull-all` is the canonical Rust build, but it also fronts three sibling implementations.
When the first argument is `go`, `bun`, or `cli`, it replaces its own process with the
matching backend and forwards every remaining argument verbatim.

```bash
pull-all go  [args]   # Go / bubbletea build      (pull-all-tui-go)
pull-all bun [args]   # Bun / ink build, JIT       (pull-all-tui-bun-jit)
pull-all cli [args]   # bash streaming version     (pull-all-repos)
```

A directory literally named `go`/`bun`/`cli` is still reachable as `pull-all ./go`, etc.

## Where the backends live

The backends live in a `pull-all-siblings/` directory next to the `pull-all` binary
(e.g. `~/bin/pull-all-siblings/`), deliberately **off `$PATH`** so they aren't top-level
commands — they're reachable only through `pull-all go|bun|cli`. The dispatcher resolves
them relative to its own location, falling back to the bare name on `$PATH` if that
directory is absent.

The `cli` backend (`pull-all-repos`, the original parallel-pull bash script that the
plain-mode output was ported from) is tracked in the repo and deployed by `make install`.
The `go` and `bun` backends are built from their own source trees.
