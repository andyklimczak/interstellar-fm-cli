#!/usr/bin/env python3
"""Capture the real TUI in xterm on a private Xvfb display.

Requires: cargo build --release; Python 3, mpv, Xvfb, xterm, xdotool, ImageMagick.
Fetches public stations from Radio Browser, uses temporary config/data, and plays
real streams through mpv's null audio output. Your own library is never opened.
"""
import argparse
import contextlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile
import time
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
API = "https://de1.api.radio-browser.info"


def stations(query, limit):
    params = urllib.parse.urlencode({"name": query, "limit": limit, "order": "votes", "reverse": "true", "hidebroken": "true", "is_https": "true"})
    request = urllib.request.Request(f"{API}/json/stations/search?{params}", headers={"User-Agent": "interstellarfm-cli/screenshots"})
    with urllib.request.urlopen(request, timeout=15) as response:
        payload = json.load(response)
    return [{
        "id": {"source": "radio_browser", "id": row["stationuuid"]},
        "name": row["name"], "url": row.get("url_resolved") or row["url"],
        "country": row.get("country") or "", "codec": row.get("codec") or "",
        "bitrate": row.get("bitrate") or 0, "votes": row.get("votes"),
        "clicks": row.get("clickcount"), "tags": row.get("tags") or "",
        "language": row.get("language") or "", "homepage": row.get("homepage") or "",
    } for row in payload if row.get("lastcheckok") == 1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "docs/screenshots")
    args = parser.parse_args()
    binary = ROOT / "target/release/interstellarfm-cli"
    if not binary.is_file():
        raise SystemExit("Run cargo build --release first")
    for command in ["mpv", "Xvfb", "xterm", "xdotool", "magick"]:
        if not shutil.which(command):
            raise SystemExit(f"Missing screenshot dependency: {command}")
    favorites = stations("SomaFM", 8) + stations("Radio Paradise", 2) + stations("Jazz24", 1)
    if len(favorites) < 4:
        raise SystemExit("Not enough public stations available for a useful demo")
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="interstellarfm-screenshots-") as temporary:
        temp = Path(temporary)
        data = temp / "data"
        data.mkdir()
        library = data / "library.json"
        library.write_text(json.dumps({"version": 1, "favorites": favorites, "history": [], "volume": 70, "muted": False}))
        wrapper = temp / "mpv-null"
        wrapper.write_text(f"#!/bin/sh\nexec {shlex.quote(shutil.which('mpv'))} --ao=null \"$@\"\n")
        wrapper.chmod(0o700)
        config = temp / "config.toml"
        config.write_text(f'mpv_path = {json.dumps(str(wrapper))}\napi_base = "{API}"\n')
        with (temp / "display.log").open("wb") as log:
            display = subprocess.Popen(["Xvfb", "-displayfd", "1", "-screen", "0", "1920x1080x24", "-nolisten", "tcp"], stdout=subprocess.PIPE, stderr=log)
            terminal = None
            try:
                number = display.stdout.readline().decode().strip()
                if not number:
                    raise RuntimeError("Xvfb failed to start")
                env = dict(os.environ, DISPLAY=f":{number}", TERM="xterm-256color")
                command = ["xterm", "-title", "Interstellar FM screenshots", "-geometry", "112x30+0+0", "-fa", "DejaVu Sans Mono", "-fs", "13", "-b", "14", "-bg", "#11111b", "-fg", "#cdd6f4", "-cr", "#cba6f7", "+sb", "-u8"]
                resources = {
                    "color0": "#11111b", "color2": "#a6e3a1", "color5": "#cba6f7",
                    "color8": "#45475a", "color13": "#cba6f7", "color15": "#f5f5ff",
                    "cursorBlink": "false", "printFileImmediate": str(temp / "screen"),
                    "printModeImmediate": "0", "printOptsImmediate": "1",
                    "VT100.translations": "#override\\n<Key>F12: print-immediate()",
                }
                for key, value in resources.items():
                    command.extend(["-xrm", f"XTerm*{key}: {value}"])
                command.extend(["-e", str(binary), "--data-dir", str(data), "--config", str(config)])
                terminal = subprocess.Popen(command, env=env, stdout=log, stderr=log)

                def xdo(*arguments):
                    return subprocess.run(["xdotool", *arguments], env=env, check=True, capture_output=True, text=True).stdout.strip()

                window = None
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    result = subprocess.run(["xdotool", "search", "--name", "^Interstellar FM screenshots$"], env=env, capture_output=True, text=True)
                    if result.returncode == 0 and result.stdout.strip():
                        window = result.stdout.splitlines()[0]
                        break
                    if terminal.poll() is not None:
                        raise RuntimeError("xterm exited before the app started")
                    time.sleep(0.1)
                if window is None:
                    raise RuntimeError("Could not find the screenshot terminal window")
                xdo("windowfocus", "--sync", window)
                xdo("mousemove", "1800", "1000")  # Keep the pointer outside the capture.

                def key(*keys):
                    xdo("key", "--clearmodifiers", *keys)
                    time.sleep(0.2)

                def screen():
                    key("F12")
                    dumps = list(temp.glob("screen*"))
                    if not dumps:
                        raise RuntimeError("xterm did not write its screen dump")
                    return max(dumps, key=lambda path: path.stat().st_mtime_ns).read_text(errors="replace")

                def wait_for(predicate, description, timeout=20):
                    deadline = time.monotonic() + timeout
                    text = ""
                    while time.monotonic() < deadline:
                        if terminal.poll() is not None:
                            raise RuntimeError("The app exited during capture")
                        text = screen()
                        if predicate(text):
                            return text
                        time.sleep(0.4)
                    raise RuntimeError(f"Timed out waiting for {description}:\n{text}")

                def capture(name):
                    time.sleep(0.3)
                    path = args.output / f"{name}.png"
                    subprocess.run(["magick", "import", "-window", window, "-strip", str(path)], env=env, check=True)
                    print(f"Captured {path.relative_to(ROOT) if path.is_relative_to(ROOT) else path}")

                wait_for(lambda text: "INTERSTELLAR FM" in text, "application startup")
                # Populate History through actual successful playback, never fabricated entries.
                for index in range(min(6, len(favorites))):
                    before = len(json.loads(library.read_text())["history"])
                    key("Home")
                    for _ in range(index):
                        key("Down")
                    key("Return")
                    try:
                        wait_for(lambda text: "PLAYING" in text and len(json.loads(library.read_text())["history"]) > before, "stream playback", timeout=25)
                    except RuntimeError as error:
                        print(f"Skipping unavailable stream: {favorites[index]['name']} ({error.args[0].splitlines()[0]})")
                    if len(json.loads(library.read_text())["history"]) >= 3:
                        break
                if len(json.loads(library.read_text())["history"]) < 2:
                    raise RuntimeError("Could not play enough stations to capture a genuine History screen")
                key("1", "Home")
                capture("saved")
                key("2", "Home")
                capture("history")
                key("3", "slash")
                xdo("type", "--clearmodifiers", "--delay", "60", "jazz")
                key("Tab")
                wait_for(lambda text: "jazz" in text and re.search(r"[1-9][0-9]* stations", text) and "searching" not in text, "live search results")
                capture("search")
                key("q")
                terminal.wait(timeout=5)
            finally:
                if terminal is not None and terminal.poll() is None:
                    terminal.terminate()
                    with contextlib.suppress(subprocess.TimeoutExpired):
                        terminal.wait(timeout=5)
                    if terminal.poll() is None:
                        terminal.kill()
                        terminal.wait()
                display.terminate()
                display.wait(timeout=5)


if __name__ == "__main__":
    main()
