# Watch a diagram

The diagram redraws on every save; a broken edit keeps the last good drawing on screen.

```mermaid
flowchart TD
    subgraph waiting [Wait for a change]
        dev([Developer runs casemap --watch FILE]) --> poll[Wait for a file event or a 1 s heartbeat]
        poll --> changed{Changed?}
        changed -->|no| poll
    end
    subgraph drawing [Update the drawing]
        render[Render it] --> ok{Valid?}
        ok -->|yes| show[Redraw the diagram]
        ok -->|no| keep[Keep last drawing, show the error]
    end
    changed -->|yes| render
    show --> poll
    keep --> poll
    %% @icon poll clock
    %% @icon render image
    %% @icon show refresh-cw
    %% @icon keep triangle-alert
```
