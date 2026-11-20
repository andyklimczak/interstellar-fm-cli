# Interstellar FM CLI

A keyboard-first Rust terminal player for internet radio. Browse **Saved**, **History**, and **Search**, discover stations with [Radio Browser](https://www.radio-browser.info/), or add your own streams and `.pls` playlists.

## Run

Requires a current stable Rust toolchain and **mpv** in your `PATH`. Linux is the initial target. macOS is not yet verified; Windows is not supported.

```bash
# Install the audio backend (Arch Linux):
sudo pacman -S mpv
# Debian / Ubuntu alternative: sudo apt install mpv

cd ~/Dev/interstellarfm-cli
cargo run --release
```

Or install the binary locally:

```bash
cargo install --path .
interstellarfm-cli
```

Useful options:

```bash
cargo run -- --help
cargo run -- --check                        # Check config, saved data, and mpv; no TUI
cargo run -- --offline                      # Disable Radio Browser; custom streams still work
cargo run -- --data-dir /tmp/my-radio-test   # Use a separate, persistent test library
cargo run -- --config ./config.example.toml
```

Run the UI in a real interactive terminal, at least 36 columns × 18 rows. It does not require a Nerd Font. Missing mpv does not prevent browsing/saving; attempting playback explains how to install it.

## Controls

| Action | Default keys |
|---|---|
| Saved / History / Search | `1` / `2` / `3` |
| Switch sidebar/table focus | `Tab` / `Shift-Tab` |
| Move | Arrows or `j` / `k` |
| First / last row | `gg` / `G`, Home / End |
| Page / half-page | PgUp / PgDn, `Ctrl-u` / `Ctrl-d` |
| Play selected station | `Enter` |
| Favorite / unfavorite selected station | `f` |
| Add stream / `.pls` | `a` |
| Remove selected favorite in Saved | `d`, then confirm |
| Play / pause current station | `p` or Space |
| Stop and disconnect | `s` |
| Volume up / down | `v` / `V`, `+` / `-` |
| Mute | `m` |
| Edit search or local Saved/History filter | `/` |
| Cycle search sort / reverse order | `o` / `O` |
| Refresh Search | `r` |
| Station details | `i` |
| Help (scroll with `j` / `k`) | `?` |
| Quit | `q` or `Ctrl-c` |

Search runs after two characters with a 350 ms debounce. An empty/one-character search shows popular stations. Sort by name, popularity, votes, or bitrate. `Tab` leaves the search/filter input and focuses the station list, preserving the query and any pending search. `Enter` submits the search immediately and focuses results; press it again to play. `Esc` leaves text entry without clearing it, `Ctrl-u` clears the field, and Backspace edits. Input currently edits at the end of the field. Bracketed paste is supported. Printable characters such as `q`, `p`, and spaces are always text while a field is active.

Saved is your favorites library. Removing a favorite does **not** stop the stream or delete its History entry. History contains the last 30 distinct stations by default and only updates once playback starts successfully. You can favorite a removed custom station again from History.

## Add your own radio

1. Press **`a`** from any section.
2. Enter a direct HTTP/HTTPS stream URL, a URL ending in `.pls`, or a local `.pls` path (including `~/...`). Use Tab to enter an optional name.
3. Press Enter to preview.
4. For a multi-entry playlist, use arrows/`j`/`k` to move, Space to select entries, or `a` to select all/none. A single entry is preselected.
5. Press **Enter to save**, or **`p` to save and play** exactly one selected entry. Esc cancels.

Example `.pls`:

```ini
[playlist]
NumberOfEntries=2
File1=https://example.com/jazz.mp3
Title1=My Jazz Radio
Length1=-1
File2=http://example.org/live
Title2=Local Radio
Length2=-1
Version=2
```

Direct URLs can be saved without testing the connection, so offline streams can still be favorited. Playlist files/URLs must be readable to import. Selected entries are stored as independent favorites; deleting the original `.pls` does not delete the favorites. Duplicate custom stream URLs are skipped. An optional name overrides a single imported entry's title; multi-entry imports retain their individual titles.

For safety, imports are limited to 1 MiB and 500 entries, with request timeouts and bounded redirects. Only HTTP/HTTPS entries are supported. Relative entries are resolved against a **remote** playlist's final URL; local playlists require absolute stream URLs. Nested `.pls` imports, local audio files, and other protocols are not supported. The app imports `.pls` itself rather than handing an entire playlist to mpv.

## Playback

mpv runs audio-only, without your personal mpv config, scripts, terminal controls, or video windows. A private socket controls the owned child. Switching stations replaces the playback session, isolating old events; the app only terminates its own processes. Quitting stops playback—this is not a background daemon.

The player shows available ICY title metadata, pause/buffering state, volume, and up to two bounded reconnect attempts. A failing Radio Browser station gets one best-effort URL refresh. Reconnects and pause/resume do not add History entries or additional click reports. Custom stations never use Radio Browser metadata/click APIs. Playback never autostarts on launch.

Live-stream pause is not DVR: buffered audio or the live edge on resume depends on the stream and mpv. There is no seek bar. Common formats are delegated to mpv; unusual streams may fail under the restricted reference/protocol policy rather than opening arbitrary external resources.

## Configuration and saved data

Copy [`config.example.toml`](config.example.toml) to:

```text
~/.config/interstellarfm-cli/config.toml
```

`$XDG_CONFIG_HOME` is respected. Configure the mpv executable, history limit, debounce, accent color, optional API override, and navigation keybindings. For example:

```toml
accent = "cyan"
history_limit = 50

[keybindings]
favorite = ["x"]
remove = ["d", "delete"]
```

Overrides replace that action's defaults; conflicting bindings and unknown settings produce an error rather than silently changing behavior. Help and the footer reflect navigation overrides. Text fields and dialogs retain their fixed controls. `gg` and `Ctrl-c` are reserved.

The entire small library is stored as one atomic snapshot to keep favorites, History, and settings consistent:

```text
~/.local/share/interstellarfm-cli/library.json
~/.local/share/interstellarfm-cli/library.json.bak
```

`$XDG_DATA_HOME` or `--data-dir` overrides this location. The snapshot contains source-aware station IDs, names/URLs, History timestamps, volume, and mute preference. The previous snapshot is retained as `.bak`. Only one running instance may use a data directory. A malformed/unsupported library is preserved and reported at startup; move it aside or restore the backup to recover.

Stream URLs (including any private tokens in them) are stored locally in plaintext. Treat library files and backups as private. Saved data is not automatically shared with the Expo app.

## Development and validation

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test live_search -- --ignored  # Optional: hits the public Radio Browser API
cargo build
python3 scripts/smoke-test.py
python3 scripts/smoke-test.py --codecs  # Also MP3/AAC/Ogg/HLS; requires ffmpeg
```

Unit tests run without public APIs or audio devices and cover state/input isolation, custom identities, storage/locking, search HTTP behavior, playlist parsing, keymap validation, and rendering at multiple terminal sizes. The opt-in Python smoke test uses a pseudo-terminal, local HTTP audio, an isolated temporary library, and real mpv with **null audio output**, so it does not play sound or modify your normal favorites. It verifies playback, pause/resume, volume/mute, favorite removal, multi-entry imports, persisted data on reload, and child cleanup. With `--codecs`, it generates MP3, AAC, Ogg, and HLS fixtures using ffmpeg and plays each through the actual TUI/backend.

## Status

This is the first usable implementation of the core workflow from [`PLAN.md`](PLAN.md), not every release-hardening item in that roadmap. It includes all three sections, Radio Browser search/popular stations with in-session mirror discovery/failover and rate-limit cooldowns, real playback, favorites/removal, History, direct streams, local/remote PLS imports, configurable navigation, and responsive layouts.

Remaining follow-ups include persistent API/popular caches, more exhaustive codec/network/platform testing, release binaries, MPRIS/media keys, mouse support, import/export interchange with the mobile app, and richer text-field cursor editing. The initial implementation deliberately uses a **single atomic library snapshot** and a **fresh mpv session per station**, rather than the split storage files and single long-lived child proposed in the plan.
