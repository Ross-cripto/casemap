# Coverage

`casemap coverage` shows which code files have at least one known `@code` anchor,
with totals by directory and the uncovered file names:

```text
code anchored to flows: 50% (2 of 4 files)

  src/auth/   50%   2/4   not covered: mfa.ts, reset.ts
```

Run `casemap coverage main` to list changed code files no flow covers since the
merge base with `main`, including uncommitted and untracked changes. Add
`--strict` to exit 1 when that list is non-empty. Repository mode has no gate.
Both modes support `--format json` (version 1).

Coverage counts files, not lines or symbols. A file is covered when it matches an
anchor to an existing step or a whole-flow (`*`) anchor, just as `casemap which`
does. With no known anchors, both modes explain how to start measuring and exit 0,
even with `--strict`.

By default code extensions are `rs`, `ts`, `tsx`, `js`, `jsx`, `mjs`, `cjs`, `py`,
`go`, `rb`, `java`, `kt`, `kts`, `swift`, `cs`, `php`, `ex`, `exs`, `scala`, `c`,
`h`, `cc`, `cpp`, `hpp`, `m`, `mm`, `dart`, `lua`, `vue`, `svelte`, `elm`, `clj`,
`fs`, `erl`, `hs`, `ml`, `zig` and `sh`. Paths under `test`, `tests`, `__tests__`,
`spec`, `specs`, `migrations`, `vendor`, `node_modules`, `target`, `dist`, `build`, `.github`
or `.git` are excluded, as are test names `*_test.*`, `*.test.*`, `*.spec.*`,
`test_*.py` and `*_spec.rb`, config names `*.config.*`, and TypeScript declarations
`*.d.ts`. Extensions and excluded folder names are matched
case-insensitively. Test files named like `*Tests.swift` or `*Test.java` outside
test folders count as code; use `--exclude` to remove them. Dotfile folders other
than `.github` and `.git` count as code by default. A module such as
`src/config.ts` also counts; its name does not match `*.config.*`.

Repeat `--include PATTERN` to replace those defaults with your own selection;
repeat `--exclude PATTERN` to remove files. Patterns use the same repository-root
syntax as anchors. Excludes always win, and catalog files are always excluded.
For example: `casemap coverage --include src --exclude src/generated`.
