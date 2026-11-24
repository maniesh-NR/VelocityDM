#!/usr/bin/env python3
"""SSE wire witness: capture the exact frames the daemon's GET /events emits.

Runs against OUR daemon on loopback (docs/RPC.md §GET /events):
1. serves a generated file over HTTP on localhost,
2. starts (or reuses) the daemon,
3. POSTs downloads.add via POST /rpc (native-client posture: JSON
   Content-Type, no Origin/Referer),
4. reads GET /events for --duration seconds and prints every frame raw.

Output is the evidence the S12-SSE-WIRING spec §0 cites. Loopback only;
nothing is uploaded anywhere.
"""
import argparse
import json
import os
import socket
import subprocess
import sys
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

FILE_BYTES = 6 * 1024 * 1024


def serve_file(port: int, path: str) -> None:
    class H(BaseHTTPRequestHandler):
        def do_GET(self):
            size = os.path.getsize(path)
            start, end = 0, size - 1
            rng = self.headers.get("Range")
            status = 200
            if rng and rng.startswith("bytes="):
                a, _, b = rng[6:].partition("-")
                if a:
                    start = int(a)
                if b:
                    end = int(b)
                status = 206
            end = min(end, size - 1)
            length = end - start + 1
            self.send_response(status)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(length))
            if status == 206:
                self.send_header(
                    "Content-Range", f"bytes {start}-{end}/{size}"
                )
            self.send_header("Accept-Ranges", "bytes")
            self.end_headers()
            with open(path, "rb") as f:
                f.seek(start)
                remaining = length
                try:
                    while remaining > 0:
                        chunk = f.read(min(65536, remaining))
                        if not chunk:
                            break
                        self.wfile.write(chunk)
                        remaining -= len(chunk)
                except (BrokenPipeError, ConnectionResetError):
                    pass  # engine aborts probe/extra segments mid-body; normal

        def log_message(self, *a):
            pass

    srv = ThreadingHTTPServer(("127.0.0.1", port), H)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    print(f"# file server on 127.0.0.1:{port} serving {path}", flush=True)
    globals()["_srv"] = srv


def wait_port(port: int, timeout: float) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=1):
                return True
        except OSError:
            time.sleep(0.2)
    return False


def rpc(addr: str, method: str, params):
    req = urllib.request.Request(
        f"http://{addr}/rpc",
        data=json.dumps(
            {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
        ).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read())


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--addr", default="127.0.0.1:36990")
    ap.add_argument("--daemon", required=True, help="path to velocitydm bin")
    ap.add_argument("--duration", type=float, default=5.0)
    ap.add_argument("--serve-port", type=int, default=8099)
    args = ap.parse_args()

    tmpfile = os.path.join(
        "/tmp", f"sse-probe-{os.getpid()}.bin"
    )
    with open(tmpfile, "wb") as f:
        f.seek(FILE_BYTES - 1)
        f.write(b"\0")

    serve_file(args.serve_port, tmpfile)

    daemon = subprocess.Popen(
        [args.daemon, "daemon", "--addr", args.addr],
        stdout=subprocess.DEVNULL,
        stderr=open("/tmp/sse-probe-daemon.log", "wb"),
    )
    try:
        if not wait_port(args.addr.split(":")[1], 20):
            print("# daemon did not come up", file=sys.stderr)
            return 2
        print(f"# daemon up at {args.addr} (pid {daemon.pid})", flush=True)

        # raw socket read of /events: no client buffering, exact wire bytes
        host, port = args.addr.split(":")
        sock = socket.create_connection((host, int(port)), timeout=10)
        sock.settimeout(0.5)
        sock.sendall(
            f"GET /events HTTP/1.1\r\nHost: {args.addr}\r\n"
            f"Accept: text/event-stream\r\n\r\n".encode()
        )

        add = rpc(
            args.addr,
            "downloads.add",
            {"url": f"http://127.0.0.1:{args.serve_port}/probe.bin",
             "options": {}},
        )
        print(f"# add reply: {add}", flush=True)

        deadline = time.time() + args.duration
        frames = 0
        buf = b""
        head_shown = False

        def drain(final: bool) -> None:
            nonlocal frames, buf
            while b"\n\n" in buf:
                frame, buf = buf.split(b"\n\n", 1)
                text = frame.decode(errors="replace")
                frames += 1
                print(f"--- frame {frames} ---\n{text}", flush=True)
            if final and buf.strip():
                print(f"--- trailing (no terminator) ---\n"
                      f"{buf.decode(errors='replace')}", flush=True)

        while time.time() < deadline:
            try:
                chunk = sock.recv(65536)
            except socket.timeout:
                continue
            except OSError:
                print("# stream reset by peer", flush=True)
                break
            if not chunk:
                print("# stream closed by peer (EOF)", flush=True)
                drain(final=True)
                break
            if not head_shown:
                head, _, rest = chunk.partition(b"\r\n\r\n")
                print(f"# response head: {head.decode(errors='replace')!r}",
                      flush=True)
                buf += rest
                head_shown = True
            else:
                buf += chunk
            drain(final=False)
            if frames >= 40:
                break
        print(f"# total frames: {frames}", flush=True)
        sock.close()
        return 0
    finally:
        daemon.terminate()
        try:
            daemon.wait(timeout=5)
        except subprocess.TimeoutExpired:
            daemon.kill()
        globals()["_srv"].shutdown()
        os.unlink(tmpfile)


if __name__ == "__main__":
    sys.exit(main())
