# Leapsofts ERP (Tauri)

Desktop shell: **ERP site** in the main window (`https://erp.leapsofts.com/projects`) plus a **time-tracker overlay** (login, check-in/out, idle/sleep, screenshots, app usage). Monitoring matches `leapsofts-erp-desktop-app/` (Electron).

## Run (macOS)

```bash
cd leapsofts-erp-app
cp .env.example .env   # already matches the Electron API URL
npm install
npm run tauri dev
```

Production build on this Mac:

```bash
npm run build          # Vite overlay
npm run tauri build
```

Rust-only check:

```bash
cd src-tauri && cargo check
```

## Permissions (macOS)

Grant these in **System Settings → Privacy & Security**, then restart the app:

- **Screen Recording** — 10-minute JPEG samples (`xcap`)
- **Accessibility** (and Input Monitoring if listed) — keyboard/mouse % (`rdev`)
- **Automation** — frontmost window titles (`active-win-pos-rs`)

First check-in usually triggers the OS prompts.

## Windows

- No extra Store permissions. Capture and idle use Win32 APIs.
- If Defender or another AV flags the global input hook (`rdev`), allow the app.
- Build: `npm run tauri build` on a Windows machine (or CI). NSIS installer is enabled in `src-tauri/tauri.conf.json`.

## Android (tracker only)

The phone app is attendance: sign-in, check-in/out, break, meeting, and offline sync. It does not capture screenshots, idle time, or which app is in front.

Needs JDK 17, Android SDK platform 36, and NDK `29.0.13846066` (the version Tauri 2.11 looks up). From this folder:

```bash
export JAVA_HOME="/path/to/jdk-17"   # JDK 17
export ANDROID_HOME="$HOME/Library/Android/sdk"
export NDK_HOME="$ANDROID_HOME/ndk/29.0.13846066"
npm run android:init    # once
npm run android:dev     # emulator or a plugged-in phone
npm run android:build   # release APK
```

The install id is `com.leapsofts.tracker`. Desktop stays `com.leapsofts.erp`.

## Linux

- Prefer **X11**. Screenshots, idle, and global input are best-effort on **Wayland** (same caveat as the Electron tracker).
- Typical packages: `libwebkit2gtk-4.1-dev`, `libxdo-dev`, `libx11-dev`, `libxtst-dev`, `libxcb-render0-dev` (names vary by distro).
- Build: `npm run tauri build` (`.deb` target is enabled).

## API / env

`VITE_API_URL` / `API_BASE` default to the same Heroku `/api/v1` URL as `leapsofts-erp-desktop-app/.env`. Overlay auth is JWT + `refreshToken` cookie stored locally (SQLite + keyring when available). The ERP webview uses its own site cookies.

## Out of scope (v1)

- Auto-update / code signing
- Replacing the Electron app
- Browser-extension URL tracking (OS active window only; hostname sanitized when a URL is present in the title)
