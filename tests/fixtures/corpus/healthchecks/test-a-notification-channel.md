# Test a notification channel

A user sends a test alert through an integration to confirm it works.

```mermaid
flowchart TD
    user([User clicks Send test notification]) --> send[Send a fake "TEST is down" alert]
    send --> noop{Channel only sends up alerts?}
    noop -->|yes| up[Send a fake "up" alert instead]
    noop -->|no| result{Sending failed?}
    up --> result
    result -->|yes| warn[Show "Could not send a test"]
    result -->|no| ok[Show "Test notification sent!"]
    warn --> list[Return to the integrations page]
    ok --> list
    %% @code * hc/front/views.py
    %% @code send hc/api/transports.py
```
