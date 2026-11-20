# Interstellar FM CLI — implementation plan

## Implementation status

The first usable Rust implementation is now in `src/`; see `README.md` for launch instructions and current controls. Core Saved/History/Search, Radio Browser discovery/search, mpv playback/reconnects, persistence, favorite removal, direct streams, local/remote PLS imports, keybinding configuration, and responsive layouts are implemented. Automated tests and an opt-in real-mpv pseudo-terminal smoke test are included.

Two intentional changes from the architecture below: favorites/history/settings use one atomic `library.json` snapshot instead of split storage files, and each station change starts a fresh owned mpv session instead of reusing a long-lived child. These simplify consistent persistence and stale playback-event isolation. Persistent discovery/popular caches, richer text-field editing, extensive codec/platform testing, and release packaging remain follow-ups. The sections below retain the original roadmap for reference, not a claim that every hardening item is complete.

## Goal

Build a keyboard-first Rust terminal radio player that feels like the terminal counterpart of `~/Dev/interstellar-fm`, with three primary sections: **Saved**, **History**, and **Search**. Keep playback visible and running while navigating between sections.

Working package/binary name: `interstellarfm-cli`. Initial platform target: Linux; keep the architecture compatible with macOS. Windows support is not an MVP requirement because the proposed mpv transport uses Unix sockets.

## References reviewed

### Interstellar FM

- `~/Dev/interstellar-fm/contexts/radio-context.tsx`: playback, persistence, volume, saved stations, and recent stations.
- `~/Dev/interstellar-fm/lib/radio-browser.ts`: API requests, mirror fallback, station normalization, search sorting, popular stations, and click reporting.
- `~/Dev/interstellar-fm/types/radio.ts`: station identity and metadata.
- `~/Dev/interstellar-fm/components/radio/`: Saved, Recent, Search, station rows, and the bottom player.

Behavior to preserve:

- Radio Browser station identity is `stationuuid`, not the name or stream URL. Custom stations use a separate stable local ID (see below).
- Saved stations (favorites) persist locally and retain their saved order, including manually added streams.
- History is most-recent-first, deduplicated by station identity, and capped at 30 by default.
- Search starts at two characters with a 350 ms debounce; shorter input shows 30 popular stations.
- Search sorting: name ascending, popularity/click count descending, votes descending, or bitrate descending, as in the original app. The CLI additionally offers Country, Language, and Codec (A–Z by default); every search sort can be reversed.
- Stream selection prefers `url_resolved` and falls back to `url`.
- Volume persists between sessions.
- Playback and saved state are shared across all sections.

### SubTUI

Reviewed the locally cached source of `https://github.com/MattiaPun/SubTUI`, especially `internal/api/config.toml`, `internal/ui/update_keys.go`, `internal/player/player.go`, and the main-view screenshot.

Borrow the interaction patterns, not its Go/Bubble Tea implementation:

- Arrow keys plus Vim navigation, page navigation, and quick jumps.
- Tab/Shift-Tab focus navigation, `/` to search, `?` for help.
- `f` for favorite, `p` for playback, and `v`/`V` for volume.
- TOML-configurable keybindings and theme colors.
- A clearly highlighted active pane and persistent bottom player.
- An external mpv process controlled through JSON IPC.

Do not bring over album browsing, queues, shuffle, repeat, seeking, scrobbling, or Subsonic accounts: they are not part of the live-radio MVP.

## 1. Interface and behavior

### Layout

Use a small navigation sidebar and a large station table, rather than three simultaneous station lists. The three sections remain visible and one section's content is active at a time.

```text
┌ Interstellar FM ──────────────────────────────────────────────┐
│ Saved (12) │ Search: jazz                     Sort: Popularity │
│ History    ├──────────────────────────────────────────────────┤
│ Search     │   Station              Country   Codec   kbps    │
│            │ > ★ Jazz Radio         France    MP3     128     │
│            │     Jazz24             USA       AAC      64     │
│            │                                                  │
├────────────┴──────────────────────────────────────────────────┤
│ PLAYING  Jazz Radio · Artist — Track, when available           │
│ LIVE · MP3 128 kbps                     Volume: 70%            │
├───────────────────────────────────────────────────────────────┤
│ Enter play · f save · p pause · / search · ? help · q quit      │
└───────────────────────────────────────────────────────────────┘
```

- Sidebar: Saved / History / Search, with counts where useful.
- Main pane: table with saved marker, station name, country, codec, and bitrate. Add last-played time in History when space allows.
- Distinguish the selected row from the currently playing station.
- Preserve each section's selection and scroll position independently.
- Search input and sort controls appear in Search; Saved/History can have a local filter instead.
- Bottom player: station, playback/buffering/error state, available ICY metadata, live indicator, volume, and mute state. Do not show a misleading track progress bar.
- Context-sensitive footer and a help overlay generated from the active keymap.
- Station details popup: country, language, tags, homepage, codec, bitrate, and stream URL.
- At narrow widths, replace the sidebar with top tabs and drop lower-priority columns. At very small sizes, show a resize message without panicking.
- Loading, empty, offline, and error states should explain what happened and how to retry.
- Dark terminal-friendly defaults, configurable accents, and an ASCII fallback; no required Nerd Font or image protocol.

### Suggested default controls

These are inspired by SubTUI, with a few radio-specific additions. Bindings apply in navigation mode unless explicitly stated otherwise.

| Action | Default keys |
| --- | --- |
| Open Saved / History / Search | `1` / `2` / `3` |
| Next / previous focusable region | `Tab` / `Shift-Tab` |
| Navigate rows or sidebar | `j` / `k`, Down / Up |
| First / last row | `gg` / `G`, Home / End |
| Page / half-page movement | PgUp / PgDn, `Ctrl-u` / `Ctrl-d` |
| Play selected station; activate sidebar item | `Enter` |
| Toggle favorite for selected station | `f` |
| Add a stream URL or `.pls` playlist to favorites | `a` |
| Remove selected favorite (Saved section only) | `d` |
| Play / pause current station | `p` or Space |
| Stop and disconnect current stream | `s` |
| Volume up / down, in 5% steps | `v` / `V`, `+` / `-` |
| Toggle mute | `m` |
| Focus Search; local filter in Saved/History | `/` |
| Cycle search sort | `o` |
| Reverse sort direction | `O` |
| Refresh current remote list / retry | `r` |
| Show selected station details | `i` |
| Close overlay / leave text input | `Esc` |
| Help | `?` |
| Quit | `q` or `Ctrl-c` |

Input rules:

- Text-editing mode consumes printable keys, including `q`, `f`, `p`, spaces, and digits; typing must never trigger playback or quit.
- `Tab` in a search/filter field focuses the results/station list, preserving the query and pending debounce without submitting again or playing a station.
- `Enter` in a search field submits immediately and focuses results; it does not immediately play a result.
- `Esc` leaves text editing without discarding the query. Clearing the query restores popular stations.
- `Ctrl-c` always exits through the cleanup path.
- Key sequence handling such as `gg` needs a timeout and must reset when focus changes.
- Configurable bindings should be validated for conflicts within their input context.

### Saved and History

- Saved is the favorites library; there is no separate Favorites section.
- Save/unsave any station from any section with `f`; changes update all visible markers immediately.
- In Saved, `d` explicitly removes the selected favorite. Confirm removal in a small dialog, then keep selection on the nearest remaining row.
- Removing a favorite never stops playback or removes its History entry. A custom station can be favorited again from History.
- Do not lose the playing station when it is removed from Saved.
- History records a station after playback successfully starts, not merely when a play command is queued. This intentionally improves on the mobile app's eager recording.
- Replaying an existing station moves it to the top; pause/resume and automatic reconnects do not create additional history entries.
- Persist `last_played_at`; history limit defaults to 30 and can be configured.
- No autoplay on startup. Restore volume and optionally the last station selection, but only connect on explicit play.

### Add a stream or `.pls` playlist directly

Include this in the MVP, without requiring a Radio Browser listing or connection.

- `a` opens an **Add favorite** dialog from any section, with a source field and optional display name.
- Accept a direct HTTP/HTTPS stream URL, an HTTP/HTTPS URL to a `.pls` playlist, or a local `.pls` file path (including `~/` expansion).
- For direct streams, validate the URL and save without requiring successful playback; saving must still work if the stream is temporarily offline. Use the entered name, or a readable URL-derived fallback.
- For `.pls`, read/fetch and parse the playlist asynchronously before saving. Recognize numbered `FileN`, optional `TitleN`/`LengthN`, and standard `[playlist]` metadata, tolerating BOMs, CRLF, missing titles, and non-contiguous entry numbers.
- Preview playable entries in playlist order. For one entry, preselect it; for multiple entries, let the user select one or more to add as individual favorites. Do not silently import or play the entire playlist.
- Prefer an explicit display name for a single selected entry, then `TitleN`, then a URL-derived fallback. Retain per-entry titles for multi-entry imports.
- Resolve relative HTTP(S) entries against the remote playlist's final URL after redirects. Local playlists must contain absolute HTTP(S) stream URLs; local media paths and other protocols are unsupported.
- Bound file/response size, redirects, entry count, and request duration. Validate every entry's scheme and refuse recursive/nested `.pls` imports in the MVP. Never pass a user-supplied `.pls` file directly to mpv for unrestricted expansion.
- Show readable errors for missing/unreadable files, malformed playlists, failed downloads, and playlists with no supported entries; allow correction or cancellation without changing favorites.
- Skip duplicate custom stream URLs and report which entries were already saved. Normalize scheme/host and default ports conservatively, preserving meaningful path/query data such as stream tokens.
- Persist the selected stream URLs and titles, plus optional original playlist source for reference. Imported favorites must keep working after the local `.pls` file is moved or deleted; automatic playlist re-import is not required.
- Saving does not autoplay or create History entries. Offer an explicit **Save and play** action for a single entry, alongside **Save** and **Cancel**.
- Use the same text-input isolation rules as Search; `Esc` cancels the dialog and its pending work, and late results cannot modify favorites.

## 2. Technical choices

| Concern | Recommendation |
| --- | --- |
| TUI | `ratatui` + `crossterm` |
| Async runtime and channels | `tokio` |
| HTTP | `reqwest` with rustls, timeouts, and bounded retries |
| Data/config serialization | `serde`, `serde_json`, `toml` |
| Argument parsing | `clap` |
| Standard user directories | `directories` or an equivalent XDG-aware crate |
| Errors and file logging | `thiserror`, `anyhow`, `tracing`, `tracing-subscriber` |
| Audio | External `mpv`, using JSON IPC over a private Unix socket |
| Tests | Ratatui TestBackend, HTTP mocks, temporary directories, fake player/IPC server |

Use a single Rust package with modules initially, not a multi-crate workspace. Select current compatible dependency versions when scaffolding and commit `Cargo.lock`.

### Why mpv rather than a native Rust decoder?

Internet radio includes MP3, AAC, Ogg, HLS, redirects, playlists, and inconsistent stream servers. mpv provides mature support without making this project implement a streaming/decoding stack. SubTUI uses the same general approach, and mpv is already installed on this machine.

Tradeoff: the app requires the `mpv` executable at runtime. Detect a missing executable and show installation guidance; browsing and saving should remain usable without playback. A native backend can be evaluated later behind the same player boundary.

## 3. Application architecture

```text
src/
  main.rs                  # CLI, startup, shutdown, terminal lifecycle
  app.rs                   # App state, typed actions, state transitions
  event.rs                 # Input, worker results, ticks, signals
  input.rs                 # Context-aware keybindings and text input
  model.rs                 # StationId/source, saved/history records, playback state
  playlist.rs              # Bounded local/remote PLS parsing and URL validation
  config.rs                # Defaults, TOML loading, validation
  storage.rs               # Versioned persistence and atomic writes
  radio_browser/
    mod.rs                 # Client and endpoint operations
    dto.rs                 # API response types and normalization
    hosts.rs               # Discovery, caching, and failover
  player/
    mod.rs                 # Commands/events and backend boundary
    mpv.rs                 # Child process, socket, JSON IPC
  ui/
    mod.rs                 # Responsive layout
    navigation.rs
    stations.rs
    search.rs
    player.rs
    overlays.rs
tests/
  fixtures/
```

- One main event loop owns mutable application state. Rendering reads state and performs no I/O.
- Typed actions produce state changes and asynchronous effects; network, disk, and player work report results through bounded channels.
- Keep the UI responsive during API timeouts, stream connection attempts, and storage writes.
- Search requests and playlist imports carry generation IDs; cancel superseded/cancelled work and reject stale responses.
- Model station identity as `RadioBrowser(uuid)` or `Custom(local_id)`, with a source discriminator and optional playlist provenance. Generate a custom ID once on addition and retain it through favorites, History, and reloads; use conservative URL matching to prevent duplicate custom imports, not as a replacement for persistent identity.
- Only Radio Browser stations use UUID metadata refresh and click-reporting endpoints. Custom stations play directly and must never generate Radio Browser API calls.
- Playback attempts also carry identities so late events from station A cannot replace the state of newer station B.
- Explicit playback states: idle, connecting, playing, paused, buffering/reconnecting, stopped, and error.
- Serialize/coalesce persistence writes so older snapshots cannot overwrite newer state.
- Use terminal restoration guards and panic handling; normal quit, errors, signals, and panic paths must restore terminal mode and clean up the owned player where possible.

## 4. Radio Browser integration

Implement the client natively in Rust, using the mobile app's API layer as the behavioral reference.

### MVP endpoints

- `GET /json/stations/search`: name search, limit 50, sort/reverse options, `hidebroken=true`.
- `GET /json/stations/topclick/30`: popular stations for an empty/short query.
- `GET /json/stations/byuuid/{uuid}`: refresh a saved station's stream metadata when needed.
- `GET /json/url/{uuid}`: best-effort click reporting after a deliberate successful station start, once per play attempt; do not report again on resume/reconnect.

### Reliability and data handling

- Verify current Radio Browser discovery guidance and endpoint contracts during implementation. Discover/cache available mirrors, randomize initial selection, reuse a healthy host, and fail over within a bounded total request deadline.
- Provide static HTTPS API mirrors as fallback and an optional configuration override for troubleshooting.
- Identify requests with `User-Agent: interstellarfm-cli/<version>`.
- Retry transient connection/server failures with bounded backoff; honor rate limiting and do not hammer all mirrors on invalid requests.
- Deduplicate results by UUID and tolerate missing/null metadata and unknown response fields.
- Remove entries without a usable stream; normalize `url_resolved` with `url` fallback and retain station identity plus descriptive metadata.
- Strip terminal control/escape sequences from station names, tags, and stream metadata before displaying them.
- Unlike the mobile search, do **not** require `is_https=true` for streams by default: a native player can play HTTP streams without browser mixed-content restrictions. Keep API traffic HTTPS and offer an HTTPS-only stream preference if wanted.
- Saved stations remain visible/playable from local data when Radio Browser is unavailable. On stale-URL failures, try refreshing station metadata once before surfacing an actionable error.
- Popular stations can be cached for a short TTL; no remote request should be required to open Saved or History.

Country, language, and tag filters plus paginated discovery are a follow-up, not necessary for the first usable release.

## 5. Playback integration

- Start one owned mpv child lazily on first play, in idle/audio-only mode, isolated from user mpv config and terminal input/output.
- Use a private per-instance runtime directory/socket; avoid a shared predictable socket and never use broad `pkill` cleanup.
- Pass arguments without a shell; send URLs through JSON IPC. Validate stream URL schemes and bound playlist handling so untrusted directory entries cannot open local files or arbitrary protocols. Manually imported `.pls` files are resolved by the application into validated stream URLs; playback must not implicitly enqueue all playlist entries.
- Correlate IPC responses by request ID and consume asynchronous property/events independently.
- Observe load/end/error, pause, buffering, volume, and metadata changes. A successful IPC load command alone does not mean audio is playing.
- Support play/replace, pause/resume, actual stop/disconnect, volume, and mute. Clamp volume to 0–100% and retain the pre-mute volume.
- Live-stream pause is best-effort buffering, not DVR. Resuming may reconnect to the live edge depending on the stream/backend.
- On connection loss, attempt a small bounded reconnect sequence while showing status; allow stop or station changes to cancel it.
- If mpv exits unexpectedly, report the failure and allow the next explicit play to start a new child.
- On quit, request graceful shutdown, then kill/reap only the owned child if needed, and remove its runtime files. No detached background playback in the MVP.

## 6. Persistence and configuration

Keep persistence simple: versioned JSON is sufficient for a saved library and a bounded history. SQLite is unnecessary for the initial feature set.

Proposed Linux paths:

```text
$XDG_CONFIG_HOME/interstellarfm-cli/config.toml
$XDG_DATA_HOME/interstellarfm-cli/library.json
$XDG_STATE_HOME/interstellarfm-cli/state.json
$XDG_STATE_HOME/interstellarfm-cli/logs/
$XDG_CACHE_HOME/interstellarfm-cli/             # Discovery/popular cache
```

Use standard home-directory fallbacks when XDG variables are unset and platform-appropriate directories on macOS.

- `library.json`: schema version, saved station snapshots, stable IDs/source types, saved timestamps, custom names, and optional playlist provenance. Custom stations do not need fabricated Radio Browser UUIDs.
- `state.json`: schema version, recent station snapshots with the same stable IDs/source types and last-played timestamps, volume, mute preference, optional last selection.
- `config.toml`: history limit, debounce, mpv path, API override, theme, keybindings, optional mouse support.
- Write via temporary file + atomic replacement and retain a recoverable backup; surface failures without crashing playback.
- Validate loaded data, clamp volume, deduplicate station identities, and preserve a corrupt file rather than silently overwriting it.
- Enforce a single running instance per state directory for MVP to avoid multi-process lost updates.
- No implicit sharing with the Expo app's AsyncStorage. JSON import/export can be added later with an explicit interchange format.

## 7. Delivery milestones

### Milestone 1 — Runnable terminal shell

- Initialize Cargo project, basic README, formatting/lint configuration, and CI.
- Implement app state/actions and the responsive layout using fixture stations.
- Add section selection, list navigation, text-input mode, help, and reliable terminal cleanup.

**Done when:** the UI can be explored with keyboard controls at several terminal sizes without network or audio.

### Milestone 2 — Radio Browser discovery

- Build the API client, DTO normalization, host discovery/failover, and HTTP mock tests.
- Wire popular stations, debounced search, seven sort choices (Name, Popularity, Votes, Bitrate, Country, Language, Codec), loading/error states, and retry.
- Ensure stale network responses cannot overwrite newer results.

**Done when:** real stations can be discovered and inspected without blocking the UI.

### Milestone 3 — Streaming vertical slice

- Implement mpv lifecycle, JSON IPC, and player state transitions.
- Wire play, pause, stop, volume/mute, metadata, bounded reconnect, and bottom player.
- Add generation-safe station switching and graceful shutdown.

**Done when:** a search result plays, navigation does not interrupt audio, and rapid station switching/quit leave no owned player behind.

### Milestone 4 — Favorites, custom streams, and History

- Implement versioned persistence, stable source-aware station identities, startup loading, and single-instance protection.
- Wire favorite/unfavorite across views, explicit removal from Saved, successful-play history recording, deduplication, history cap, and volume restore.
- Implement the Add favorite dialog for direct stream URLs and local/remote `.pls` files, with bounded parsing, entry selection, optional naming, and duplicate detection.
- Handle offline startup and corrupt/unwritable storage visibly and safely.

**Done when:** Radio Browser and custom favorites, History, and volume survive restarts; users can import playlist entries and remove favorites without interrupting playback; locally known stations remain usable during API outages.

### Milestone 5 — Release hardening

- TOML keybinding/theme configuration, details overlay, local filters, and narrow-terminal polish.
- Complete failure-path tests, manual stream matrix, install instructions, and Linux release builds.
- Verify macOS separately before advertising it as supported.

**Done when:** the acceptance checklist below passes and a clean machine can install/run the documented binary with mpv.

## 8. Testing and acceptance

Automate:

- State transitions: save/unsave/remove, confirmation/cancellation, stable row selection, recent ordering/cap, volume/mute, input-mode isolation, and custom station IDs retained after unfavorite/refavorite through History.
- Custom favorites: direct URL validation, optional naming, duplicate detection, no implicit autoplay/history, persistence across restart, and no Radio Browser requests.
- PLS fixtures: local/remote sources, BOM/CRLF, missing fields, non-contiguous numbering, multiple-entry selection, relative remote URLs, duplicates, malformed/oversized input, unsupported schemes, nested playlists, and missing files. Verify imports survive deletion of their source file.
- Search debounce/cancellation and playlist-import cancellation/stale-result rejection using controlled time.
- HTTP behavior: URL encoding, sort parameters, missing fields, normalization, failover, timeouts, rate limits, empty results.
- Player behavior using a fake backend/IPC server: interleaved events/responses, load failure, station replacement, reconnect cancellation, child exit, and shutdown.
- Persistence round trips, schema handling, corruption recovery, write failures, and instance locking.
- Ratatui buffer snapshots at normal, narrow, and tiny sizes, including long/Unicode/control-character metadata and empty lists.
- CI: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`; keep live network/audio tests opt-in.

Manual release checks:

- Discover and play representative MP3, AAC, Ogg, and HLS stations, including redirects/playlist URLs.
- Switch sections while playing, rapidly change stations, stop during connect/reconnect, and quit while buffering.
- Disconnect/reconnect the network; simulate API downtime independently of stream availability.
- Verify real live-stream pause/resume behavior and ICY metadata updates.
- Add a direct stream, import both local and remote `.pls` files, select entries from a multi-entry playlist, and verify duplicate feedback and error/cancel behavior.
- Remove a playing favorite with `d` (and another with `f`); verify playback/History remain intact and refavoriting from History works.
- Restart and confirm Radio Browser/custom favorites, History, names, and volume persist; delete an imported source `.pls` and verify its saved streams still play; verify no autoplay.
- Verify missing mpv, read-only storage, a second instance, and terminal resize all produce useful behavior.
- Verify terminal restoration and child/socket cleanup on normal quit, Ctrl-C, and handled failure paths.

## 9. Follow-up scope

After the core experience is reliable:

- Country/language/tag filters and paginated search.
- Linux MPRIS/media keys, analogous to the mobile app's system playback controls and SubTUI's desktop integration.
- Optional desktop notifications and mouse controls.
- Station library import/export, including a documented bridge from Interstellar FM exports.
- Custom ordering, richer history, optional artwork, and additional playlist formats such as M3U.
- Background daemon/remote control or scripting subcommands only if there is a concrete need.

Do not block the MVP on these. The first release should do four things well: **discover a station, play it reliably, save it, and find it again in History**.
