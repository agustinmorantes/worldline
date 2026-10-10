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

This narrows ADR 0004: icons stop being device-local facts. The index still caches the local icon path.

## Considered options

- **The GUI computes the hue from the local icon.** Rejected: a device without the icon, or with a different one, shows a different color for the same game.
- **Store the hue alone in `GameMeta` and keep icons local.** Rejected: an adopted game would have the right color but no icon.
