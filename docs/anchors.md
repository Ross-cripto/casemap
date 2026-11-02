# Anchors: keep flows true

Tie steps to the code that implements them with Mermaid comments inside the
flow's first Mermaid block, usually at the end before the closing fence:

```text
%% @code pay src/checkout/payment.ts
%% @code confirm src/checkout/confirmation.ts tests/confirmation.ts
%% @code * src/checkout
```

A target is a node id, or `*` for the whole flow. Several patterns on one line
and repeated lines for the same target add up. `casemap flows NAME` shows them
in a Code footer. Mermaid and GitHub ignore the comments; rendering is unchanged.
Anchors in prose or any other block are errors.

Paths are relative to the repository root and always match from that root:

- A literal file matches itself; a literal folder matches everything under it.
- `*` matches within one path segment and `?` matches one character in a segment.
- `**` as a whole segment matches zero or more folders: `src/**/pay.rs` also
  matches `src/pay.rs`. In `src/**.rs`, the stars behave like `*.rs`.
- Every other character is literal, including brackets and braces:
  `app/[id]/page.tsx` matches that exact framework path.
- A leading `./` and trailing `/` are removed; repeated `/` separators collapse.
  Absolute paths, drive prefixes, `.` or `..` segments, and backslashes are rejected.
- Patterns are separated by whitespace, so anchor paths cannot contain spaces.

`casemap check` reports `anchor-syntax`, `anchor-unknown-step`, and
`anchor-no-match` as errors. It indexes tracked and untracked files through git,
excluding ignored files and files deleted from disk. Outside git, or without the
git binary, it walks the current directory without following directory symlinks.
Update anchors when code moves or is renamed.

`casemap diff` compares anchored paths with code changed since the merge base,
including deleted files, both sides of a rename, and non-ignored untracked files.
It appends an advisory section when a flow file was untouched:

```text
possibly stale: 1 flow whose anchored code changed but the flow did not
  Pay for an order (pay.md)
    pay: src/checkout/payment.ts
```

A flow is touched when any byte in its file changed between the base and the
working tree. A touched flow is never reported stale, even when only its prose
or anchors changed. Review stale flows: update them when behaviour changed; if
they still hold, report that instead of making a cosmetic edit to silence the warning.

`casemap diff --strict` also exits 1 for possibly stale flows. Use it for local
agent runs or teams that require acknowledgement of every anchored change.
**`casemap check --strict` remains the recommended CI gate.** Running
`diff --strict` in CI requires `actions/checkout` with `fetch-depth: 0` so git
can find the merge base.

Run `casemap which src/checkout/payment.ts` before editing to find anchored flows
and steps. It only knows anchored flows. Arguments are literal paths relative to
the current directory, or absolute paths; directories expand to indexed files,
and deleted paths still match anchors. Paths outside the repository are rejected.
Pass several paths for separate result blocks, or `--format json` for JSON v1:

```json
{"version":1,"paths":[{"path":"src/pay.rs","flows":[{"name":"pay","path":"docs/flows/pay.md","title":"Pay for an order","steps":["pay"],"whole_flow":false}]}]}
```
