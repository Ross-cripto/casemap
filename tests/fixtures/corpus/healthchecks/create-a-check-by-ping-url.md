# Create a check by pinging its slug

A job pings a project's ping key and slug with create=1, and a new check appears.

```mermaid
flowchart TD
    job([Job pings the slug URL with create=1]) --> upper{Slug is all lowercase?}
    upper -->|no| badurl[Reply "invalid url format"]
    upper -->|yes| exists{Check with that slug exists?}
    exists -->|yes| ping[Record the ping]
    exists -->|no| create{Ping URL has create=1?}
    create -->|no| notfound[Reply "not found"]
    create -->|yes| limit{Project is within twice its check limit?}
    limit -->|no| notfound
    limit -->|yes| made[Create the check and assign all channels]
    made --> ping
    ping --> created[Reply "Created"]
    %% @code * hc/api/views.py
    %% @code made hc/api/models.py
```
