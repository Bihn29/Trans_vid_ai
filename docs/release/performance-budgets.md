# Performance budgets

Reference environment is the pinned Windows CI runner with release optimizations where applicable.

| Operation | Budget | Measurement |
| --- | ---: | --- |
| Rust application-state foundations | 5 s | database migrations, project layout, recovery metadata |
| Queue restart recovery | 2 s | recover 1,000 persisted interrupted jobs |
| SHA-256 of 64 MiB artifact | 3 s | generated local fixture, no network or filesystem cache assertion |
| UI interaction reducer | 100 ms | deterministic store transition benchmark |
| External worker shutdown after cancellation | 2 s | process-tree test deadline |

Budgets are regression guards, not hardware guarantees. Media encoding and AI inference depend on codecs, models, CPU/GPU, and input duration; release notes must report measured profiles rather than claim a universal runtime. A budget change requires recorded evidence and must not hide a security or correctness regression.
