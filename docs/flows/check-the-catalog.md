# Check the catalog

A developer or an agent validates every flow before committing, locally or in CI.

```mermaid
flowchart TD
    subgraph validation [Validate the flows]
        who([Agent or CI runs casemap check]) --> read[Read every flow file]
        read --> rules{Every flow valid?}
    end
    subgraph outcome [Report the result]
        ok[Report checked N flows: ok]
        report[List each problem with file and line] --> strict{Errors, or warnings with --strict?}
        strict -->|yes| fail[Exit 1]
        strict -->|no| pass[Exit 0]
    end
    rules -->|yes| ok
    rules -->|no| report
    %% @icon who bot
    %% @icon read file
    %% @icon ok check
    %% @icon report list
    %% @icon fail x
    %% @icon pass check
    %% @code rules src/check.rs src/anchors.rs
```
