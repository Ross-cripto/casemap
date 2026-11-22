# Map a new repository

A developer runs casemap in a repository without flows and lets a coding agent write them.

```mermaid
flowchart TD
    subgraph starting_point [Find a starting point]
        dev([Developer runs casemap in a terminal]) --> map{Any flows yet?}
        map -->|no| agents{Coding agent installed?}
        agents -->|no| steps[Print the steps to map it by hand]
        agents -->|yes| menu{Developer picks an option}
        menu -->|steps| steps
    end
    subgraph authoring [Create the map]
        init[Create the catalog, install the skill] --> work[Agent writes flows with the developer]
        work --> check[Check the new flows]
        check --> written{Any flow written?}
        written -->|no| retry[Explain how to try again]
    end
    subgraph browsing [Open the map]
        browse[Open the browser]
    end
    map -->|yes| browse
    menu -->|an agent| init
    written -->|yes| browse
    %% @icon steps list
    %% @icon init folder-open
    %% @icon work bot
    %% @icon check scan
    %% @icon retry refresh-cw
    %% @icon browse map
    %% @code * src/onboard.rs
    %% @code init src/main.rs
```
