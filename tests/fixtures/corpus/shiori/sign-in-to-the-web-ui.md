# Sign in to the web UI

A user opens Shiori, enters a username and password and lands in their library.

```mermaid
flowchart TD
    open([User opens a Shiori page]) --> authed{Already signed in?}
    authed -->|yes| library[Show the library]
    authed -->|no| form[Show the login form]
    form --> creds[Enter username and password]
    creds --> check{Credentials accepted?}
    check -->|yes| library
    check -->|no| failed[Show "Login failed" message]
    failed --> creds
    %% @code authed webapp/src/router/index.ts
    %% @code creds webapp/src/views/LoginView.vue
    %% @code check internal/http/handlers/api/v1/auth.go
    %% @code failed webapp/src/stores/auth.ts
```
