---
status: accepted
date: 2026-10-09
amended: 2026-10-09 (z-order: at the game's level, not topmost)
---

# The game overlay is a separate window, driven from Rust

Worldline shows a small popup over the game when something happens to its saves: a checkpoint
was kept, a capture failed, another device is ahead. How it gets on screen decides whether it can break games,
anti-cheat or frame pacing, so the shape is fixed here.

## Decision

- **No injection.** The overlay is a separate transparent, click-through Tauri window.
  Nothing of ours runs inside the game process: no `Present` hook, no DLL. Cost: nothing shows
  over true exclusive fullscreen, and that is accepted.
- **Popup-sized and hidden when idle.** The window is only as big as the popup and is hidden
  between popups, never a full-window layer and never shown at zero opacity. Any visible pixel of
  another window over a borderless game makes Windows compose the frame, which turns off
  G-Sync/FreeSync and adds latency. The one exception is the naming card, which covers the game
  while the player types, because the player asked for it.
- **At the game's z-order, not topmost.** The overlay sits directly above the game window, in
  the game's band: topmost only when the game itself is topmost. A window the user brings in front
  of the game covers the overlay too. While a popup shows, Rust puts it back above the game if the
  game rises over it. A topmost overlay was seen drawn over a terminal in front of the game,
  which is why. Verified with stand-in windows only (a normal and a topmost window
  as the game, a decoy window in front); real games are unverified.
- **Only over the game, only while it's in front.** At show time the foreground window must
  belong to a process running from the game's install dir. The popup hides at once on a
  foreground change, a move, a resize or a minimise.
- **Never takes focus, except Name it.** Passive popups are non-activating. The Name it hotkey
  takes focus for the card and gives it back to the game when the card closes.
- **Exists only while a tracked game runs.** The window is created on the first `GameStarted` of
  a game with an install dir and destroyed when the last one stops. Between popups its WebView2
  memory target is `Low`.
- **All decisions in Rust.** `gui/src-tauri/src/overlay/` owns the gate, priority, timers,
  placement, hotkeys and sound. The overlay webview only renders the payload it receives and
  reports when its exit animation ends.
- **Prefs reach Rust by push.** The GUI owns the prefs schema and sends the overlay config with
  `set_overlay_config` after loading prefs and on every change. Rust never parses `prefs.json`.
  Copy stays in GUI i18n (ADR 0007): anything Rust must write in the app language, such as the
  automatic tag, comes in that config.

## Considered options

- **Hook the game's `Present` (Steam/Discord legacy style).** Works in exclusive fullscreen, but
  anti-cheat can ban for it, it crashes games, and it needs one implementation per graphics API.
  Rejected.
- **Make the game window the overlay's owner** (`GWLP_HWNDPARENT`). Keeps the overlay above the
  game without topmost, but linking windows across processes ties their input queues together,
  so a stall in either process can freeze the other. Rejected.
- **Topmost.** The first choice here: simplest, and safe with games that make their own window
  topmost. Rejected after testing: it draws over windows the user puts in front of the game. The
  game's own topmost case is handled by matching the game's band instead.
- **A full-window transparent layer kept over the game** (Discord's 2025 overlay). Simplest to
  draw into, but keeps the game composed the whole time. Rejected.
- **Rust reads `prefs.json` directly.** Removes the push, but gives the prefs schema two owners.
  Rejected.

## Consequences

- Exclusive-fullscreen games get no popup. The hotkeys still work there: Quick keep with sound
  only, Name it through the main window.
- Linux is out for now: Wayland can't guarantee always-on-top, and Steam Deck Gaming Mode is
  gamescope. The overlay code is `cfg(windows)` and a no-op elsewhere.
- Full-screen captures (streams) show the popup; window captures don't. Quiet mode exists for
  that.
- A game that raises itself often makes the overlay restack often; a game that is topmost in odd
  ways may hide it. Both are open until checked in real games. Unconditional topmost is not an
  automatic or permitted fallback: if the z-order fails there, the user decides what changes.
