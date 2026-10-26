# Watch a diagram

The diagram redraws on every save; a broken edit keeps the last good drawing on screen.

```mermaid
flowchart TD
    dev([Developer runs casemap --watch FILE]) --> poll[Wait for a file event or a 1 s heartbeat]
    poll --> changed{Changed?}
    changed -->|no| poll
    changed -->|yes| render[Render it]
    render --> ok{Valid?}
    ok -->|yes| show[Redraw the diagram]
    ok -->|no| keep[Keep last drawing, show the error]
    show --> poll
    keep --> poll
```
