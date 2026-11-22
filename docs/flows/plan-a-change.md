# Plan a change

A developer and their agent review how a planned change alters a use case before any code is written.

```mermaid
flowchart TD
    subgraph proposal [Draft the proposed flow]
        plan([Agent plans a change]) --> write[Write the proposed flow]
        write --> preview[Compare the proposal with the flow]
        preview --> valid{Proposal passes check?}
        valid -->|no| errors[List the problems in the proposal]
        errors --> write
    end
    subgraph implementation [Review and implement]
        review{Developer approves the plan?} -->|yes| save[Write the flow, then the code]
        save --> anchor[Flag new steps with no code anchor]
    end
    valid -->|yes| review
    review -->|no| write
    %% @icon plan bot
    %% @icon write file
    %% @icon preview git-compare
    %% @icon errors list
    %% @icon save code
    %% @icon anchor triangle-alert
    %% @code preview src/main.rs
    %% @code anchor src/flows.rs
```
