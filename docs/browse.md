# Browse

`casemap browse [--base REF]` opens a full-screen catalog browser. The default base
is `HEAD`; `--base main` compares against its merge base with HEAD. Use `--flows
DIR`, `--ascii`, `--no-color` or `--dir TD|BT|LR|RL` as needed. `NO_COLOR` also
selects a plain UI; `--color` has no effect here.

An excerpt of the 120×30 `browse__wide` fixture snapshot:

```text
┌ Flows ───────────────────────┐Pay for an order
│✗ Bad flow                    │tests/fixtures/flows/browse/pay-for-an-order.md
│✗ Broken flow                 │A customer pays for an order and receives a receipt.
│~ Checkout                    │┌ Diagram ─────────────────────────────────────────────────────────────────────────────┐
│· Login                       ││    ╭───────────╮                                                                     │
│! Pay for an order            ││    │ Customer  │                                                                     │
│+ Refund                      ││    ╰─────┬─────╯                                                                     │
│· Wide flow                   ││          │                                                                           │
│- Legacy export               ││          ▼                                                                           │
```

| Key | List focus | Diagram focus |
|---|---|---|
| ↑/↓, k/j | Select a flow | Pan one row |
| ←/→, h/l | | Pan one column |
| H/J/K/L | | Pan ten columns/rows |
| PgUp/PgDn | Select by page | Pan by visible height |
| Home | First flow | Reset pan |
| Tab | Focus diagram; open it on narrow terminals | Focus list |
| Enter | Open diagram on narrow terminals | |
| / | Search title or name | Search title or name |
| d | Toggle diff against the base | Same |
| p | Toggle problems, code anchors and diff details | Same |
| r | Reload flows, checks and anchored code | Same |
| ? | Help | Help |
| q, Ctrl-C | Quit | Quit |
| Esc | Close help → finish search → clear kept filter → narrow list → quit | Same |

Search accepts printable characters, including command letters. ↑/↓ still
select; Backspace deletes; Enter keeps the filter, or clears an empty query.
Ctrl-C quits even while searching. Changed flow files reload automatically;
edits to anchored code outside the catalog refresh on `r`. When git is unavailable
or HEAD has no commit, the status says `no git` and diff mode is disabled.

Markers, in priority order: `✗` (`x` in ASCII) means a parse/check error, `!`
possibly stale, `~` modified, `+` new, `-` removed, and `·` (`.` in ASCII) unchanged.
Removed flows appear last. Diagrams pan without wrapping. Below 80 columns,
Tab switches between the list and diagram views.

Bare `casemap` opens the browser only when stdin **and** stdout are terminals,
`CI` is unset, `TERM` is not `dumb`, the catalog (`--flows` or `docs/flows`) has flows,
and neither FILE nor `--watch` is supplied. With no flows it offers setup;
outside an interactive session it reads stdin as before.
Explicit `casemap browse` requires terminal stdout. Build with
`--no-default-features --features cli` for the CLI without the browser; library
embedders can use `default-features = false`.
