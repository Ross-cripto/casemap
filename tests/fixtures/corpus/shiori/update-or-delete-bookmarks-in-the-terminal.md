# Update or delete bookmarks in the terminal

A person re-downloads, edits or removes saved bookmarks, or checks which ones are dead links.

```mermaid
flowchart TD
    run([User runs shiori update, delete or check]) --> all{Indices given?}
    all -->|no| ask[Ask Continue? y/N for ALL bookmarks]
    ask --> yes{Answered y?}
    yes -->|no| nothing[Print No bookmarks updated]
    yes -->|yes| which{Which command?}
    all -->|yes| which
    which -->|update| upd[Print download progress]
    which -->|delete| del[Print Bookmark(s) have been deleted]
    which -->|check| chk[Print each bookmark as reachable or not]
    chk --> sum[Print the list of unreachable bookmarks]
    %% @code ask internal/cmd/delete.go
    %% @code upd internal/cmd/update.go
    %% @code chk internal/cmd/check.go
```
