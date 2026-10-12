---
status: accepted
date: 2026-10-11
---

# Game icons may be fetched from Steam

Steam keeps only a 32 px icon per app on disk (`appcache/librarycache/{appid}/<community_icon hash>.jpg`). Valve serves a 184 px version of the same icon without an API key, so Worldline downloads it.

## Decision

- Core may download `https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/{appid}/{hash}_full.jpg`, and `{hash}.jpg` (32 px) when Steam has no `_full` one.
- A game that still has no icon after the install fill (ADR 0005) gets its Steam icon. Core looks its title up in the Ludusavi manifest by exact normalized title (titles that normalize alike but name different appids are skipped), then asks `IStoreBrowseService/GetItems` for every such appid at once (100 per call). This runs where the install fill runs, after it, so an install's icon always wins: in a sync after the pull and before the push, and right after a scan when no remote is configured.
- Only appids that were asked about and 40-hex hashes ever go into a URL; nothing else from Steam's answer is fetched.
- This is the first outbound call besides the Ludusavi manifest. It sends Steam appids and icon hashes to Valve, never anything personal.
- It is always on; there is no setting.
- A download that fails is never an error: core uses the local 32 px icon. Only a 404 or 410 is an answer (many older apps have no `_full` icon on any Steam CDN host), so the Add Games preview caches it; refusals, server errors and bodies that aren't images are tried again on a later scan. When there is no answer at all (offline), the scan stops downloading for the rest of that run and doesn't cache the fallback, so a later scan can still upgrade it.
- A Steam icon wins over the install's `.ico` or exe icon, even when Steam only has 32 px. The "largest exe" heuristic often lands on `UnityCrashHandler64.exe` or similar, whose icon is not the game's.
- Lookups that found nothing (no appid, no `community_icon`, neither icon size) are kept in the device-local `cache/icon_lookups.json` and not repeated for 7 days. A lookup that failed for now (no manifest downloaded yet, offline, throttled, refused, a server error, an answer that isn't Steam's, an icon download that isn't an image) is not kept, so the next sync tries again. While offline, that costs one failing call per sync.
- A game whose `auto` icon is smaller than 256 px (ADR 0005) is looked up the same way, and takes Steam's icon only when it is larger with no smaller side (amended 2026-10-11). Every lookup that got an answer, a sharper icon or not, is kept for 7 days like a miss, so a game whose best Steam icon is 184 px is asked about weekly, not every sync. A game with a 256 px icon, or a `user` one, is never asked about.

## Considered options

- **A setting, default on.** Rejected: appids aren't personal, and the setting would only hide icons.
- **Prefer a larger exe icon over a 32 px Steam one.** Rejected for now (see above). Revisit if exe selection gets reliable.
- **Keep every failed lookup for 7 days**, outages included. Rejected: one sync while offline would hide every missing icon for a week.
- **Search Steam by title** for games the manifest doesn't name. Rejected: a fuzzy match can put another game's icon on a game, and it sends titles to Valve.
