---
status: accepted
date: 2026-10-07
---

# The GUI owns its copy, its error messages and the language

The app ships in English and Spanish. Core's errors are English `to_string()` text, and the tray strings were hard-coded in Rust.

- **All user-facing text lives in the GUI.** Core never formats a message for display. `get_startup_notice` returns a path, and the GUI words it.
- **Errors:** every call to core can fail. The GUI shows its own localized message for each call and never core's text. The raw error goes to a log file (`tauri-plugin-log`), and error toasts offer "Copy details". Core exposes a coarse error kind only where the GUI needs to tell failures apart: sync folder unreachable, and game is running.
- **Language is a GUI preference.** Rust never knows it. The tray's labels and its notification text are pushed by the GUI through `set_tray_labels`.
- **i18n has no library.** `en.ts` is the source of truth. Other languages are typed against it, so a missing key or a wrong parameter fails to compile. Plurals use `Intl.PluralRules`, and strings with parameters are functions. Community translation tooling is not a goal; new languages are generated and reviewed.

## Considered options

- **i18next or Lingui.** Rejected: they add runtime lookups, string keys checked only at runtime, and file formats aimed at translation platforms the project doesn't use.
- **Translate core's errors.** Rejected: messages written for developers make poor UI copy even once translated.
