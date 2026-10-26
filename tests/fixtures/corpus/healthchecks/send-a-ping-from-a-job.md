# Send a ping from a job

A cron job or script reports that it ran, failed or started by calling its check's ping URL.

```mermaid
flowchart TD
    job([Job requests the ping URL]) --> known{Check exists?}
    known -->|no| notfound[Reply "not found"]
    known -->|yes| status{Exit status above 255?}
    status -->|yes| badurl[Reply "invalid url format"]
    status -->|no| rid{Run id is a valid UUID?}
    rid -->|no| baduuid[Reply "invalid uuid format"]
    rid -->|yes| record[Record the ping and update the check]
    record --> ok[Reply "OK"]
    %% @code known hc/api/views.py
    %% @code record hc/api/models.py
    %% @code ok hc/api/views.py
```
