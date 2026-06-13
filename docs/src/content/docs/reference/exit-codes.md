---
title: Exit codes
description: How polygit reports the run outcome through its process exit code.
---

`polygit` sets its process exit code from the run outcome, so scripts and CI can branch
on it.

| Code | Meaning |
|------|---------|
| `0` | All repos succeeded (updated or already up to date). |
| `1` | At least one repo failed. |
| `2` | The user quit mid-run before all repos finished. |
| `130` | Interrupted with `Ctrl`+`C`. |

## Example

```bash
if polygit --no-tui ~/projects; then
  echo "all clean"
else
  echo "something failed or was interrupted (exit $?)"
fi
```
