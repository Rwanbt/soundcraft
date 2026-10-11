#!/usr/bin/env python3
"""Start the real native SoundCraft GUI and exercise the loopback JSON-lines protocol.

Run inside xvfb-run, after cargo build -p soundcraft --locked.
No real sound hardware or external port is required.
"""

import json
import os
from pathlib import Path
from queue import Empty, Queue
import socket
import subprocess
import sys
import threading
import time

BINARY = Path("target/debug/soundcraft")
STARTUP_SECONDS = 90
REQUEST_SECONDS = 15


def request(stream, method, ident, params=None):
    payload = json.dumps({"id": ident, "method": method, "params": params or {}})
    stream.sendall((payload + "\n").encode("utf-8"))
    received = bytearray()
    while b"\n" not in received:
        block = stream.recv(4096)
        if not block:
            raise RuntimeError("desktop closed control connection before replying")
        received.extend(block)
        if len(received) > 8 * 1024 * 1024:
            raise RuntimeError("desktop returned an unexpectedly large control frame")
    result = json.loads(received.split(b"\n", 1)[0])
    if result.get("id") != ident:
        raise RuntimeError(f"wrong response id: {result!r}")
    if result.get("ok") is not True:
        raise RuntimeError(f"{method} failed: {result!r}")
    return result.get("result")


def main():
    if not BINARY.is_file():
        raise RuntimeError(f"missing desktop binary: {BINARY}")
    env = dict(os.environ)
    env["SOUNDCRAFT_NO_PREFS"] = "1"
    env["LIBGL_ALWAYS_SOFTWARE"] = "1"
    env["RUST_LOG"] = "warn"
    proc = subprocess.Popen(
        [str(BINARY), "--demo", "--no-audio", "--control", "0"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        bufsize=1,
        env=env,
    )
    messages = Queue()
    logs = []

    def drain_output():
        if proc.stdout is not None:
            for line in proc.stdout:
                logs.append(line.rstrip("\n"))
                messages.put(line)

    reader = threading.Thread(target=drain_output, name="soundcraft-log-reader", daemon=True)
    reader.start()
    try:
        port = None
        deadline = time.monotonic() + STARTUP_SECONDS
        while time.monotonic() < deadline:
            if proc.poll() is not None:
                raise RuntimeError(f"desktop exited before opening control port: {proc.returncode}")
            try:
                line = messages.get(timeout=min(1.0, max(deadline - time.monotonic(), 0.05)))
            except Empty:
                continue
            if "SOUNDCRAFT_CONTROL_PORT=" in line:
                port = int(line.split("SOUNDCRAFT_CONTROL_PORT=", 1)[1].strip())
                break
        if not port or not (1 <= port <= 65535):
            raise RuntimeError("desktop did not announce its loopback control port")
        with socket.create_connection(("127.0.0.1", port), timeout=REQUEST_SECONDS) as conn:
            conn.settimeout(REQUEST_SECONDS)
            session = request(conn, "session.inspect", 1)
            if not isinstance(session, dict) or not isinstance(session.get("tracks"), list):
                raise RuntimeError(f"invalid real desktop session: {session!r}")
            view = request(conn, "ui.inspect", 2)
            if not isinstance(view, dict) or "window_size" not in view:
                raise RuntimeError(f"desktop view did not initialize: {view!r}")
            command_list = request(conn, "engine.commands", 3)
            if not isinstance(command_list, list) or not command_list:
                raise RuntimeError("desktop command catalog is empty")
            print(f"PASS real desktop startup on 127.0.0.1:{port}; tracks={len(session['tracks'])}; commands={len(command_list)}")
            request(conn, "app.quit", 4)

        try:
            exit_code = proc.wait(timeout=30)
        except subprocess.TimeoutExpired as error:
            raise RuntimeError("desktop did not exit after app.quit") from error
        if exit_code != 0:
            raise RuntimeError(f"desktop returned exit code {exit_code}")
        print("PASS desktop shutdown via real control command")
    except Exception:
        print("Last desktop log lines:\n" + "\n".join(logs[-70:]), file=sys.stderr)
        raise
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=8)


if __name__ == "__main__":
    main()
