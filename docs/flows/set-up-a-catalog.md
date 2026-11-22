# Set up a catalog

A developer runs casemap init in a repository and gets a flow catalog and the agent skill installed.

```mermaid
flowchart TD
    subgraph catalog [Set up the catalog]
        dev([Developer runs casemap init]) --> dir{Catalog exists?}
        dir -->|no| create[Create the catalog folder]
    end
    subgraph installation [Install the agent skill]
        skill{Skill already installed?} -->|no| install[Install the skill for each agent]
        skill -->|same version| keep[Leave it as is]
        skill -->|different| force{--force given?}
        force -->|yes| install
        force -->|no| warn[Keep it and say how to update]
    end
    subgraph outcome [Finish setup]
        next[Print the next steps]
    end
    dir -->|yes| skill
    create --> skill
    install --> next
    keep --> next
    warn --> next
    %% @icon create folder-open
    %% @icon install download
    %% @icon keep check
    %% @icon warn circle-help
    %% @icon next list
    %% @code install src/skill.rs skills/casemap/SKILL.md
```
