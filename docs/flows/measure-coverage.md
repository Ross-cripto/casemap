# Measure coverage

A developer or an agent finds the code that no use-case flow covers, in the whole repository or in a branch.

```mermaid
flowchart TD
    subgraph scope [Choose what to measure]
        who([Developer or agent runs casemap coverage]) --> anchors{Any flow anchored to code?}
        anchors -->|no| hint[Explain how to anchor steps]
        anchors -->|yes| mode{Base ref given?}
    end
    subgraph repository [Measure the repository]
        repo[Group code files by directory] --> share[Show the share each flow covers]
    end
    subgraph branch [Review branch coverage]
        changed[List code changed since the base] --> gaps{Any changed code uncovered?}
        gaps -->|yes| list[List the uncovered files]
        gaps -->|no| ok[Say every change is covered]
    end
    mode -->|no| repo
    mode -->|yes| changed
    %% @icon hint circle-help
    %% @icon repo folder-open
    %% @icon share chart-bar
    %% @icon changed git-compare
    %% @icon list list
    %% @icon ok check
    %% @code * src/coverage.rs
```
