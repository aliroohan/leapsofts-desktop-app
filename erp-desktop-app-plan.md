# ERP Desktop App — Tauri Build Plan

## Overview

Desktop app (Tauri) that wraps the Leapsofts ERP (`https://erp.leapsofts.com`) and adds:
- Local check-in / check-out
- Screenshot monitoring
- App/URL activity tracking

**Stack:** Tauri v2 (Rust backend + system webview), local SQLite for offline state, REST sync to existing cloud API.

---

## Architecture

```
Tauri Window
 ├─ Main webview -> https://erp.leapsofts.com
 └─ Overlay/tray UI -> check-in status, controls

Rust backend (async, tokio)
 ├─ screenshot scheduler
 ├─ active window/idle tracker
 ├─ local SQLite (attendance + activity queue)
 └─ sync task -> cloud API (reqwest)
```

---

## Phase 1 — Core shell

- [ ] Scaffold Tauri v2 project
- [ ] Main `WebviewWindow` pointing at `https://erp.leapsofts.com/projects`
- [ ] Configure `tauri.conf.json`: title, icon, CSP
- [ ] Verify ERP session/auth persists across app restarts (cookies vs localStorage/JWT — confirm which the ERP uses)
- [ ] App packaging + code signing pipeline working end-to-end (Windows first, then macOS/Linux if needed)

## Phase 2 — Check-in / Check-out

- [ ] Decide UI surface: overlay webview vs system tray menu (lean tray for v1 — simpler)
- [ ] Rust command: `check_in()` / `check_out()` -> calls cloud API via `reqwest`
- [ ] Local SQLite table for attendance state (survives restarts/offline)
- [ ] Sync queued check-in/out events when back online
- [ ] Gate monitoring (screenshots/tracking) to only run while checked in

## Phase 3 — Screenshot monitoring

- [ ] Add `xcap` crate (cross-platform screen capture)
- [ ] `tokio::time::interval` scheduler (e.g. every 5–10 min, consider randomized jitter)
- [ ] Save to local temp dir -> upload via `reqwest` multipart -> delete local copy after confirmed upload
- [ ] Test Linux/Wayland behavior early (known inconsistent permission model vs X11)
- [ ] Handle multi-monitor setups

## Phase 4 — App/URL tracking

- [ ] Active window title tracking: `active-win` crate (or platform APIs: Win32 `GetForegroundWindow`, macOS `NSWorkspace`, X11 `_NET_ACTIVE_WINDOW`)
- [ ] Idle detection: `user-idle` crate — pause tracking when idle
- [ ] In-app webview navigation tracking (URLs inside the Tauri app itself — easy, via Tauri nav events)
- [ ] **Decision needed:** do we need URLs from the user's regular browser (Chrome/Firefox)? If yes, this requires a companion browser extension reporting to a local Tauri HTTP/WebSocket server — scope this as a separate sub-project, confirm requirement before building

## Phase 5 — Sync & offline handling

- [ ] Local event queue (SQLite) for attendance + screenshots + activity logs
- [ ] Background sync task, interval-based, to cloud API
- [ ] Offline queueing + flush-on-reconnect logic
- [ ] Retry/backoff strategy for failed uploads

## Phase 6 — Overlay/status UI (optional, can run parallel to Phase 2)

- [ ] Small always-on-top webview or tray menu showing: check-in status, elapsed time, monitoring on/off
- [ ] Evaluate Tauri v2 multi-webview support for overlay vs separate small window

---

## Open questions / decisions to confirm

- Auth: does the ERP use cookie-based sessions or token-based (JWT in localStorage)? Affects persistence setup.
- Do we need browser URL tracking outside the app, or is in-app + active-window-title tracking enough?
- Screenshot interval: fixed vs randomized? Any resolution/compression requirements for upload size?
- Target OSes: Windows only, or also macOS/Linux? (Affects Wayland screenshot handling, idle-detection APIs)
- Data retention: how long are screenshots/activity logs kept server-side?

---

## Key crates

| Purpose | Crate |
|---|---|
| Screenshot capture | `xcap` |
| Active window tracking | `active-win` |
| Idle detection | `user-idle` |
| HTTP client | `reqwest` |
| Local DB | `tauri-plugin-sql` (SQLite) |
| Async runtime | `tokio` |
