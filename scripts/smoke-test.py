#!/usr/bin/env python3
"""Opt-in Linux TUI/playback smoke test; uses local HTTP and mpv's null audio output.

Run after cargo build: python3 scripts/smoke-test.py
No public radio server, speakers, or changes to your normal library are needed.
"""
import argparse
import contextlib
import fcntl
import http.server
import io
import json
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import socket
import struct
import subprocess
import tempfile
import threading
import time
import wave
from urllib.parse import urlsplit

BINARY = Path(__file__).resolve().parents[1] / "target/debug/interstellarfm-cli"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codecs", action="store_true", help="Also test MP3, AAC, Ogg, and HLS (requires ffmpeg)")
    args = parser.parse_args()
    if not BINARY.exists():
        raise SystemExit("Run cargo build first")
    mpv = shutil.which("mpv")
    if not mpv:
        raise SystemExit("Install mpv first")
    with tempfile.TemporaryDirectory(prefix="interstellarfm-smoke-") as temp:
        root = Path(temp)
        audio = io.BytesIO()
        with wave.open(audio, "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(8000)
            wav.writeframes(b"\0\0" * 8000 * 120)
        streams = {"/live.wav": ("audio/wav", audio.getvalue())}
        if args.codecs:
            ffmpeg = shutil.which("ffmpeg")
            if not ffmpeg:
                raise SystemExit("Install ffmpeg to use --codecs")
            common = [ffmpeg, "-nostdin", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100", "-t", "8"]
            for ext, codec in [("mp3", "libmp3lame"), ("aac", "aac"), ("ogg", "libvorbis")]:
                subprocess.run(common + ["-c:a", codec, str(root / f"test.{ext}")], check=True)
            subprocess.run(common + ["-c:a", "aac", "-f", "hls", "-hls_time", "1", "-hls_list_size", "0", str(root / "test.m3u8")], check=True)
            for path in root.glob("test*"):
                mime = "application/vnd.apple.mpegurl" if path.suffix == ".m3u8" else "application/octet-stream"
                streams[f"/{path.name}"] = (mime, path.read_bytes())

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                entry = streams.get(urlsplit(self.path).path)
                if entry is None:
                    self.send_error(404)
                    return
                mime, payload = entry
                self.send_response(200)
                self.send_header("Content-Type", mime)
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                with contextlib.suppress(BrokenPipeError, ConnectionResetError):
                    self.wfile.write(payload)

            def log_message(self, *_args):
                pass

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        stream = f"http://127.0.0.1:{server.server_port}/live.wav"
        wrapper = root / "mpv-null"
        wrapper.write_text(f'#!/bin/sh\necho $$ >> "{root}/player-pids"\nexec "{mpv}" --ao=null "$@"\n')
        wrapper.chmod(0o700)
        config = root / "config.toml"
        config.write_text(f'mpv_path = "{wrapper}"\n')
        library_path = root / "data/library.json"
        transcript = bytearray()
        pid, fd = pty.fork()
        if pid == 0:
            os.environ["TERM"] = "xterm-256color"
            os.execv(str(BINARY), [str(BINARY), "--offline", "--data-dir", str(root / "data"), "--config", str(config)])
        fcntl.ioctl(fd, 0x5414, struct.pack("HHHH", 32, 110, 0, 0))  # TIOCSWINSZ
        exited = False

        def pump(seconds=0.25):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                ready, _, _ = select.select([fd], [], [], 0.05)
                if ready:
                    try:
                        transcript.extend(os.read(fd, 65536))
                    except OSError:
                        return

        def send(text):
            os.write(fd, text.encode())
            pump()

        def library():
            try:
                return json.loads(library_path.read_text())
            except (FileNotFoundError, json.JSONDecodeError):
                return {}

        def player_property(name):
            try:
                player_pid = (root / "player-pids").read_text().splitlines()[-1]
                command = Path(f"/proc/{player_pid}/cmdline").read_bytes().decode().split("\0")
                ipc = next(arg.split("=", 1)[1] for arg in command if arg.startswith("--input-ipc-server="))
                with socket.socket(socket.AF_UNIX) as client:
                    client.settimeout(1)
                    client.connect(ipc)
                    client.sendall((json.dumps({"command": ["get_property", name], "request_id": 99}) + "\n").encode())
                    with client.makefile() as response:
                        for line in response:
                            value = json.loads(line)
                            if value.get("request_id") == 99:
                                return value.get("data")
            except (OSError, ValueError, StopIteration, IndexError):
                return None

        def wait_for(predicate, label):
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                pump(0.1)
                if predicate():
                    return
            raise AssertionError(f"Timed out: {label}\nTerminal output:\n{transcript.decode(errors='replace')}")

        try:
            pump(0.5)
            send("a")
            send(f"\x1b[200~{stream}\x1b[201~")
            send("\tSmoke radio")
            send("\r")
            wait_for(lambda: b"Choose favorites" in transcript, "add preview")
            send("p")
            wait_for(lambda: len(library().get("history", [])) == 1, "successful mpv playback/history")
            assert len(library()["favorites"]) == 1
            wait_for(lambda: any(bar.encode() in transcript for bar in "▁▂▃▄▅▆▇█"), "playing equalizer")
            send("p")
            wait_for(lambda: player_property("pause") is True, "pause")
            wait_for(lambda: "Ⅱ".encode() in transcript, "pause indicator")
            send("p")
            wait_for(lambda: player_property("pause") is False, "resume")
            send("+")
            wait_for(lambda: player_property("volume") == 75, "volume")
            send("m")
            wait_for(lambda: player_property("mute") is True, "mute")
            send("m")
            wait_for(lambda: player_property("mute") is False, "unmute")
            assert len(library()["history"]) == 1
            send("d")
            send("n")
            assert len(library()["favorites"]) == 1
            send("d")
            send("\r")
            wait_for(lambda: library().get("favorites") == [], "favorite removal")
            assert len(library()["history"]) == 1
            # Import two entries, select all, save without interrupting playback.
            pls = root / "stations.pls"
            pls.write_text(f"[playlist]\nFile1={stream}?one\nTitle1=One\nFile3={stream}?two\nTitle3=Two\n")
            transcript.clear()
            send("a")
            send(f"\x1b[200~{pls}\x1b[201~")
            send("\r")
            wait_for(lambda: b"Choose favorites" in transcript, "PLS preview")
            send("a")
            send("\r")
            wait_for(lambda: len(library().get("favorites", [])) == 2, "multiple PLS favorites")
            pls.unlink()
            assert len(library()["history"]) == 1  # Import did not autoplay.
            if args.codecs:
                for count, ext in enumerate(["mp3", "aac", "ogg", "m3u8"], start=2):
                    transcript.clear()
                    send("a")
                    send(f"\x1b[200~http://127.0.0.1:{server.server_port}/test.{ext}\x1b[201~")
                    send("\r")
                    wait_for(lambda: b"Choose favorites" in transcript, f"{ext} preview")
                    send("p")
                    wait_for(lambda: len(library().get("history", [])) == count, f"{ext} playback")
                    print(f"PASS: {ext} playback through TUI and mpv")
            send("s")
            send("q")
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                child, status = os.waitpid(pid, os.WNOHANG)
                if child:
                    exited = True
                    assert os.waitstatus_to_exitcode(status) == 0
                    break
                pump(0.1)
            assert exited, "TUI did not exit"
            check = subprocess.run([str(BINARY), "--check", "--data-dir", str(root / "data"), "--config", str(config)], capture_output=True, text=True, check=True)
            assert f"{6 if args.codecs else 2} favorites, {5 if args.codecs else 1} history entries" in check.stdout
            assert library()["volume"] == 75
            for player_pid in (root / "player-pids").read_text().splitlines():
                try:
                    os.kill(int(player_pid), 0)
                except ProcessLookupError:
                    continue
                raise AssertionError(f"Owned mpv child still running: {player_pid}")
            print("PASS: TUI input, custom stream playback, history, removal/cancel, multi-entry PLS import, persistence, and child cleanup")
        finally:
            if not exited:
                with contextlib.suppress(ProcessLookupError):
                    os.kill(pid, signal.SIGTERM)
                with contextlib.suppress(ChildProcessError):
                    os.waitpid(pid, 0)
            os.close(fd)
            server.shutdown()


if __name__ == "__main__":
    main()
