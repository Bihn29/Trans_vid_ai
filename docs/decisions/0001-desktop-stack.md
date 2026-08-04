# ADR 0001: Tauri, React, and Rust desktop stack

- Status: Accepted
- Date: 2026-08-01

## Context

The application needs a native desktop lifecycle, controlled media subprocesses, resumable local state, and a productive editor UI without shipping a second browser runtime.

## Decision

Use Tauri 2 on Rust stable for the desktop host and React with strict TypeScript and Vite for the UI. Rust is the sole owner of SQLite, filesystem mutation, artifacts, queue state, and child processes. The UI communicates through narrow typed Tauri commands.

## Consequences

The installer is smaller than an Electron design and process authority is centralized. Windows builds require the MSVC toolchain and WebView2. Frontend and Rust contracts need explicit versioning; Tauri IPC is not a bypass around domain validation.

