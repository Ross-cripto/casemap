# Review flow changes

After an agent's task, the developer sees which use cases changed and how.

```mermaid
flowchart TD
    subgraph comparison [Compare with git]
        agent([Agent finishes a task]) --> update[Update the flows it changed]
        update --> dev[Developer runs casemap diff main]
        dev --> base[Find the merge base with git]
        base --> compare[Compare each flow with its base version]
    end
    subgraph result [Show the changes]
        any{Any flow changed?} -->|no| none[Print no flow changes]
        any -->|yes| report[Summarise and draw each changed flow]
    end
    compare --> any
    %% @icon agent bot
    %% @icon update file
    %% @icon dev terminal
    %% @icon base git-branch
    %% @icon compare git-compare
    %% @icon report image
    %% @code base src/git.rs
    %% @code compare src/diff.rs src/flows.rs
```
