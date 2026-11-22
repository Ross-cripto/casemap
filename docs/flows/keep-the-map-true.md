# Keep the map true

A coding agent finishing a task is stopped once when its changes left the use-case map out of date.

```mermaid
flowchart TD
    subgraph inspection [Check the pending changes]
        agent([Agent is about to finish a task]) --> active{Already continued by a hook?}
        active -->|no| look[Check the uncommitted changes]
        look --> found{Any warnings?}
        found -->|yes| seen{Same warnings as last time?}
    end
    subgraph outcome [Finish or respond]
        done[Let the agent finish]
        stop[Stop the agent with the warnings] --> fix[Agent updates flows or explains]
    end
    active -->|yes| done
    found -->|no| done
    seen -->|yes| done
    seen -->|no| stop
    %% @icon agent bot
    %% @icon look scan
    %% @icon done check
    %% @icon stop triangle-alert
    %% @icon fix message-square
    %% @code * src/hook.rs
```
