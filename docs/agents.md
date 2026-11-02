# Agents

## Set up

Run `casemap` in your repository. With no flows yet, it offers to have Claude Code or Codex write them, then opens the browser.

For manual setup, run `casemap init`.

Ask your coding agent: **"document the use-case flows of this repo"**.

`casemap init` creates `docs/flows/` with a `.gitkeep` and installs the
`casemap` skill for Claude Code, Codex and other agents. It keeps existing
flows and differing skills; use `casemap init --force` to update those skills.

## The agent skill

The `casemap` skill teaches how to identify use cases, group entry points
into user journeys, write the [flow format](flow-format.md), and maintain affected flows in the
same change as the code. It also teaches how to check the files and report what
changed, including open questions about behaviour that was not verified.

`casemap init` installs the skill at the repository root in
`.claude/skills/casemap/SKILL.md` for Claude Code and
`.agents/skills/casemap/SKILL.md` for Codex and other agents. From a
subdirectory, it still installs skills at the root while creating the catalog
relative to the current directory. The installed skill records that catalog's
path relative to the root. Run casemap from the root when following the skill.

To install only the skill through the skills CLI:

```sh
npx skills add Ross-cripto/casemap
```

Both installers use the source at `skills/casemap/SKILL.md`. `casemap init`
prints a reminder when git ignores an installed skill, so you can make it
available to teammates. It follows symlinked skill directories, including those
created by the skills CLI.

For agents to keep flows current on every task, add this line to `AGENTS.md`
and `CLAUDE.md` (init prints it, and leaves those files for you to edit):

```text
Use-case flows live in docs/flows/ (casemap skill): update them when behaviour changes and run `casemap check`.
```

For agents without skill support, `casemap guide` prints the skill body.
`casemap guide --flows flows` renders its paths for a custom catalog.

## Plan a change first

Have the agent write the proposed flow, then preview it against the current one with `casemap propose <name>`: it reads stdin and writes nothing, so a heredoc works in Claude Code or Codex plan mode. Put the preview in the plan or spec (Spec Kit's spec or Kiro's requirements); after approval, write the flow first, then the code.

The flow stays in the catalog after the change, so it can guide later work instead of disappearing with a per-change spec.

```text
Checkout (checkout.md)  modified: +1 step, -1 step, ~1 step, +3 links, -3 links, ~1 link
  + step fraud "Fraud check"
  ~ step confirm "Order confirmed" -> "Order confirmed and emailed"
  - step address "Enter address"
  + link cart -> pay
  + link pay -> fraud "yes"
  + link fraud -> confirm
  ~ link pay -> error "no" -> "declined"
  - link cart -> address
  - link address -> pay
  - link pay -> confirm "yes"

proposal: ok
```

The drawing is omitted above. In catalogs that use anchors, `diff` and the PR comment list added steps without an anchor as "no @code anchor yet".

## Agent hooks

`casemap hook stop` checks uncommitted changes against `HEAD` when a coding
agent is about to finish. It reports catalog check errors, possibly stale
flows whose anchored code changed, and changed code no flow covers (when the
catalog has known anchors). It blocks once per new set of warnings, remembering
them per session in the git directory. The agent can update flows, anchor code,
or explain why the flow still holds or no flow applies. The same warnings do not stop it
twice; a clean state clears the memory. Internal errors silently let the agent
finish. If writing the warning memory fails, the hook still blocks and may
repeat the warnings on the next stop.

Install the Stop hook for both Claude Code and Codex:

```sh
casemap init --hooks
```

This merges the hook into `.claude/settings.json` and `.codex/hooks.json`,
preserving existing settings and hooks. Adding the entry rewrites the JSON
with 2-space indentation and sorted keys. An existing casemap Stop command
leaves the file untouched; use `--force` to replace that command, for example after changing `--flows`.
Custom catalog paths are quoted only when needed; quoted paths assume a POSIX shell.
Codex asks you to trust project hooks once: run `/hooks` in Codex.

For manual setup, merge this JSON into **each** of `.claude/settings.json`
(Claude Code) and `.codex/hooks.json` (Codex):

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          { "type": "command", "command": "casemap hook stop", "timeout": 30 }
        ]
      }
    ]
  }
}
```

For a custom catalog, use `casemap init --hooks --flows docs/use-cases`, or
set the command to `casemap hook stop --flows 'docs/use-cases'` manually.

## pre-commit

You can also validate flows with the [pre-commit](https://pre-commit.com/)
framework. Install casemap on `PATH` and add this to `.pre-commit-config.yaml`
(pin `rev` to a release or commit containing the hook definition):

```yaml
repos:
  - repo: https://github.com/Ross-cripto/casemap
    rev: main
    hooks:
      - id: casemap-check
```
