# Release security exceptions

Review date: 2026-08-02. Owner: release engineering. Exceptions expire on 2026-09-02 or immediately when a compatible upstream fix exists, whichever comes first.

| Advisory | Severity | Locked path and exposure | Decision |
| --- | --- | --- | --- |
| [RUSTSEC-2026-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194) | high | `quick-xml 0.38.4 -> plist 1.8.0 -> tauri-utils`; duplicate-attribute parsing is reachable only in Tauri/plist build and bundle tooling. VietDub's Windows runtime does not accept XML/plist input. `plist 1.8.0` constrains `quick-xml` to the incompatible `0.38` line. | Temporarily pass this exact ID to `cargo audit --ignore`; monitor `plist`/Tauri and upgrade as soon as a compatible release exists. |
| [RUSTSEC-2026-0195](https://rustsec.org/advisories/RUSTSEC-2026-0195) | high | Same build-only path; VietDub never invokes `NsReader` over user data. | Same exact, temporary exception; no wildcard or severity-wide ignore. |
| [GHSA-g7r4-m6w7-qqqr](https://github.com/advisories/GHSA-g7r4-m6w7-qqqr) | low | `esbuild 0.27.7` is transitive through Vite 7.3.5 and affects the Windows development server. The production desktop bundle contains static output and no development server; `devUrl` binds loopback. | `pnpm audit --audit-level high` remains the release gate. Re-evaluate when Vite supports `esbuild >=0.28.1`; do not force an unsupported transitive override. |

`time` was upgraded to 0.3.47 to resolve RUSTSEC-2026-0009. Vite was upgraded to 7.3.5 and Vitest to 3.2.6 to resolve all npm high/critical findings. RustSec informational warnings for GTK3 crates are non-Windows target dependencies; other unmaintained/unsound warnings are monitored but are not disclosed vulnerabilities in the shipped Windows path.
