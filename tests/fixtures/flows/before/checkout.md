# Checkout

A customer pays for the cart.

```mermaid
flowchart TD
    cart([Customer opens cart]) --> address[Enter address]
    address --> pay{Payment ok?}
    pay -->|yes| confirm[Order confirmed]
    pay -->|no| error[Show payment error]
```
