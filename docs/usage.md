# Usage guide

## Discovery and navigation

- `1`, `2`, `3` open Saved, History, and Search. Tab/Shift-Tab switch sidebar/table focus.
- Move with arrows or `j`/`k`; Home/End or `gg`/`G` jump to the first/last row.
- PgUp/PgDn move a page; Ctrl-u/Ctrl-d move a half-page in navigation mode.
- `/` edits the search query, or a local filter in Saved/History. Tab focuses results without clearing the query; Enter submits immediately and focuses results. Press Enter again to play.
- Text fields currently edit at the end. Backspace edits, Ctrl-u clears, and Esc leaves the field. Bracketed paste is supported. Printable shortcuts such as `q` never trigger actions while typing.
- Search starts at two characters with a 350 ms debounce. Shorter input browses 30 stations; named searches return up to 50.
- `o` cycles **Clicks → Name → Click trend → Votes**, with Votes selected by default. `O` reverses the order. Name defaults to A–Z; the other options default to highest first. `r` refreshes/retries Search.

Votes indicate explicit community approval. Clicks indicate Radio Browser station
selections, **not actual listener counts**; neither guarantees audio quality.
The table uses rounded compact counts: `23,059` → `23k`, `2,300` → `2.3k`, and
`1,200,000` → `1.2m`. Press `i` for exact counts and codec/bitrate details.
Narrow terminals prioritize Votes and hide Clicks/country when needed.

## Favorites and History

`f` toggles the selected favorite from any section. In Saved, `d` removes a favorite
after confirmation. Removing a favorite never stops playback or deletes History.

History records successful playback, keeps the last 30 distinct stations by default,
and does not add entries for pause/resume or automatic reconnects. Favorite a removed
custom station again from History to restore it.

Favorites, History, volume, and mute persist across restarts. Counts are last-known
snapshots, refreshed when a station appears in a new directory response. Custom streams
and older saved entries show `—` for unknown counts rather than zero.

## Add a stream or playlist

1. Press `a` and enter an HTTP/HTTPS stream URL, a URL ending in `.pls`, or a local `.pls` path (`~/...` is supported).
2. Use Tab to enter an optional name; Enter opens a preview.
3. Select playlist entries with Space; `a` selects all/none. A single entry is preselected.
4. Enter saves; `p` saves and plays exactly one selected entry; Esc cancels.

```ini
[playlist]
NumberOfEntries=2
File1=https://example.com/jazz.mp3
Title1=My Jazz Radio
File2=http://example.org/live
Title2=Local Radio
Version=2
```

Direct URLs can be saved while offline. Playlist sources must be readable during import.
Selected streams become independent favorites, so deleting the original file does not
break them. Duplicate custom URLs are skipped. An optional name overrides a single
entry's title; multiple entries keep their individual titles.

Imports are limited to 1 MiB and 500 entries, with bounded redirects and request timeouts.
Only HTTP/HTTPS streams are supported. Relative URLs resolve against a remote playlist's
final URL; local playlists require absolute stream URLs. Nested `.pls` imports, local
audio files, and other protocols are unsupported.

## Playback

mpv runs audio-only, isolated from your personal mpv config/scripts. The app owns its
player processes and private sockets. A new session replaces the previous station;
quitting stops playback. There is no background daemon or autoplay on launch.

`p`/Space pauses/resumes, `s` stops and disconnects, and `m` toggles mute without losing
the volume setting. Live-stream pause is not DVR: resume behavior depends on the stream
and mpv. The player displays available ICY title metadata and buffering status, and
attempts up to two bounded reconnects. Radio Browser stations get a best-effort URL
refresh on failure; custom stations never use directory metadata/click APIs.

Missing mpv does not prevent browsing/saving. Unusual streams may fail under the
restricted protocol policy. Linux is the initial target; macOS is unverified and
Windows is unsupported. Use a terminal at least 36 columns × 18 rows; no Nerd Font is needed.

## Configuration and data

Copy [config.example.toml](../config.example.toml) to
`~/.config/interstellarfm-cli/config.toml`. Settings include the mpv executable,
history limit, debounce, accent, API override, and navigation shortcuts.

```toml
accent = "cyan"
history_limit = 50

[keybindings]
favorite = ["x"]
remove = ["d", "delete"]
```

Overrides replace an action's defaults; unknown settings/conflicting keys are errors.
Help and the footer reflect navigation overrides. Dialog/text-field controls are fixed;
`gg` and Ctrl-c are reserved. `$XDG_CONFIG_HOME` is respected.

The atomic library snapshot is at
`~/.local/share/interstellarfm-cli/library.json`, with the previous snapshot in
`library.json.bak`. `$XDG_DATA_HOME` or `--data-dir` overrides the location. Only one
instance may use a data directory. Corrupt/unsupported libraries are preserved and
reported; restore the backup or move the damaged file aside to recover.

**Stream URLs and any private tokens in them are stored in plaintext. Keep library
files and backups private.** Data is not automatically shared with the mobile app.

Useful options:

```bash
interstellarfm-cli --check
interstellarfm-cli --offline                      # Disable Radio Browser only
interstellarfm-cli --data-dir /tmp/my-radio-test  # Isolated library
interstellarfm-cli --config ./config.example.toml
```

Persistent API caches, MPRIS/media keys, mouse controls, mobile-app import/export,
and richer cursor editing remain follow-ups. See [PLAN.md](PLAN.md) for the roadmap.
