# Sign in with an email link

A user asks for a sign-in link, gets it by email, follows it and lands on their projects.

```mermaid
flowchart TD
    user([User opens the login page]) --> email[Enter email address]
    email --> valid{Address accepted?}
    valid -->|no| again[Show the form errors]
    again --> email
    valid -->|yes| sent[Show "login link sent" page]
    sent --> follow[User opens the link in the email]
    follow --> cookie{Browser has the login cookie?}
    cookie -->|no| confirm[Show a button to confirm sign-in]
    confirm --> token{Link is still valid?}
    cookie -->|yes| token
    token -->|no| bad[Show login page with bad-link notice]
    token -->|yes| twofa{Account has two-factor sign-in?}
    twofa -->|yes| second[Ask for the second factor]
    twofa -->|no| in[Land on the projects page]
    %% @code email hc/accounts/views.py
    %% @code token hc/accounts/views.py
    %% @code twofa hc/accounts/views.py
    %% @code email hc/accounts/forms.py
```
