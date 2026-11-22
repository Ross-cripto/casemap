# Review a pull request

A reviewer opens a pull request and sees which use cases it changed, drawn in a casemap comment.

```mermaid
flowchart TD
    subgraph reporting [Generate the report]
        pr([Developer opens or updates a PR]) --> run[Action runs casemap diff on the PR]
        run --> ok{Report written?}
        ok -->|no| fail[Fail with the casemap error]
        ok -->|yes| summary[Write the job summary]
    end
    subgraph commenting [Update the comment]
        own{casemap comment exists?} -->|yes| update[Update that comment]
        own -->|no| news{Anything to report?}
        news -->|yes| post[Post a new comment]
        news -->|no| skip[Post nothing]
    end
    subgraph outcome [Set the check result]
        errors{Check errors?} -->|yes| red[Fail the pull request check]
        errors -->|no| green[Pass the pull request check]
    end
    summary --> own
    update --> errors
    post --> errors
    skip --> errors
    %% @icon run git-compare
    %% @icon fail triangle-alert
    %% @icon summary list
    %% @icon update message-square
    %% @icon post message-square
    %% @icon red x
    %% @icon green check
    %% @code * action.yml
    %% @code run src/flows.rs src/mermaid.rs
```
