<p align="center">
  <img src="https://osvyn-dadd6.vercel.app/logo.png" alt="Osvyn logo" width="420">
</p>

# Osvyn

Multi-account manager for Muse on Windows 10 and 11. Osvyn runs many
Muse accounts at the same time in a single desktop app. Every account
gets its own profile with a separate, persistent login session, and
profiles never interfere with each other.

## Features

- Isolated profiles with separate cookies, history, and context
- DeepSleep: profiles idle for 3 minutes release their RAM and wake
  with one click
- QuickSwitch: jump between profiles with Ctrl+1 to Ctrl+9
- Aura ID and Presence Dot to tell profiles apart at a glance
- Profile Vault: export and import profiles to move between machines
- Hub: one space where profiles from all accounts gather
- Interface in 14 languages
- One-time MuseKey activation per machine, no subscription

## Plans

| Plan | Profiles | Feature updates |
| --- | --- | --- |
| Founding | 3 | 6 months |
| Personal | 6 | 12 months |
| Bundle Studio | 10 | 24 months |
| Hub | Unlimited | 24 months |

Product page: https://osvyn-dadd6.vercel.app

## Repository layout

- `src/` holds the interface pages (profile bar, Hub, activation,
  splash) and the download bridge script
- `src-tauri/` holds the Rust application, capabilities, and the
  installer configuration
- `docs/` holds the product landing page

## Building

The app is built with Tauri 2 and Rust. On Windows, install the
Microsoft C++ Build Tools and run:

```text
cargo tauri build
```

The installer is produced as an NSIS setup executable.

## License

Proprietary software. All rights reserved. This repository is shared
for reference; redistribution of the application or its source is not
permitted.
