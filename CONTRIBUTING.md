# Contributing

Bug reports and pull requests are welcome. Include reproduction steps, terminal size,
OS, and mpv version when reporting playback or rendering issues. Redact private stream
URLs/tokens and avoid sharing your library file without checking its contents.

## Development

Use a current stable Rust toolchain. Run locally with `cargo run`; use
`--data-dir /tmp/interstellarfm-dev` to keep development data separate:

```bash
cargo run -- --data-dir /tmp/interstellarfm-dev
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Default tests do not need public APIs or audio devices. Optional integration checks:

```bash
cargo test live_search -- --ignored  # Public Radio Browser API
cargo build
python3 scripts/smoke-test.py        # Real TUI + mpv, local HTTP, silent output
python3 scripts/smoke-test.py --codecs  # Also MP3/AAC/Ogg/HLS; requires ffmpeg
```

The smoke test uses a temporary library and null audio output. CI runs formatting,
Clippy, unit tests, and the codec smoke test on Linux.

## README screenshots

Install `mpv`, `xorg-server-xvfb`/`xvfb`, `xterm`, `xdotool`, and ImageMagick, plus
a monospace font such as DejaVu Sans Mono. Then run:

```bash
cargo build --release
python3 scripts/screenshots.py
```

This captures Saved, History, and Search from the actual application running in xterm
on a private Xvfb display. It fetches public Radio Browser stations, plays streams with
mpv's **null audio output** to populate genuine History, and searches for jazz. Your
normal library/config are never opened. Internet access and available streams are
required; counts and metadata naturally change between captures.

Images are saved in `docs/screenshots/`. Review them before committing; do not include
private URLs or desktop windows. See [PLAN.md](docs/PLAN.md) for implementation context and
the remaining roadmap.
