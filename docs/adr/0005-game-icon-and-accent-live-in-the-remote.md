---
status: accepted
date: 2026-10-07
---

# Game icon and accent color live in the remote

The app tints itself with a per-game accent hue taken from the game icon's dominant color. Icons were device-local (ADR 0004 lists them with the device-local facts), so two devices could show the same game with different icons and different colors.

The icon becomes shared game data:

- The store keeps `icon.png`, normalized to 256 px, in the game's folder.
- `GameMeta` gains `icon_sha256`, `accent_hue` and `accent_hue_source` (`auto` or `user`).
- Core computes the hue from the icon whenever the icon changes and the source is `auto`. A user-chosen color sets the source to `user` and survives later icon changes. With no icon, the hue is derived from the game UUID.
- Concurrent edits resolve last-writer-wins on `GameMeta.updated_at`, like the rest of the game meta.
- Bundles carry the icon.
- On adding or adopting a game, the remote icon wins. The install scanner's icon is only used when the remote has none.
- A game without an icon gets one from any device where it is installed (amended 2026-10-11). The device fills it from its install scan: during a sync after the pull and before the push, so the write can't win last-writer-wins over a rename another device made, and right after the scan when no remote is configured. The fill rechecks under the repository lock that the game still exists, has the same title and still has no icon, so it never replaces a remote or user icon. An icon that can't be had right now is tried again on a later fill. At startup, one background scan runs when some game has no icon, and a sync in a process that hasn't scanned yet scans once first. A game still without an icon after that gets its Steam icon (ADR 0010).

This narrows ADR 0004: icons stop being device-local facts. The index still caches the local icon path.

## Considered options

- **The GUI computes the hue from the local icon.** Rejected: a device without the icon, or with a different one, shows a different color for the same game.
- **Store the hue alone in `GameMeta` and keep icons local.** Rejected: an adopted game would have the right color but no icon.
