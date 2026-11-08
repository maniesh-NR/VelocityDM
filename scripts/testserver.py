#!/usr/bin/env python3
"""Range-capable test file server for the VelocityDM E2E script.

Optional env knobs:
  VDM_RTT_MS   simulated per-connection handshake latency (applied once per
               TCP connection on its first request; models WAN RTT cost of
               TCP/TLS setup). Only paths under /rtt/ pay the penalty.
"""
import http.server
import json
import os
import re
import socketserver
import sys
import threading
import time

ROOT = sys.argv[1] if len(sys.argv) > 1 else "."
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8765
RTT_MS = float(os.environ.get("VDM_RTT_MS", "0"))

_stats_lock = threading.Lock()
_stats = {"connections": 0, "requests": 0}


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    first_request = True

    def setup(self):
        super().setup()
        with _stats_lock:
            _stats["connections"] += 1

    def log_message(self, fmt, *args):
        pass

    def _resolve(self):
        path = self.path.split("?")[0]
        if path == "/":
            path = "/index.html"
        return os.path.join(ROOT, path.lstrip("/"))

    def do_HEAD(self):
        self._maybe_rtt()
        f = self._resolve()
        if not os.path.isfile(f):
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        size = os.path.getsize(f)
        self.send_response(200)
        self.send_header("Content-Type", self.guess(f))
        self.send_header("Content-Length", str(size))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("ETag", '"e2e"')
        self.end_headers()

    def _maybe_rtt(self):
        """Simulate WAN latency for /rtt/ paths.

        Every request pays RTT_MS (request head -> first response byte);
        the first request on a fresh connection pays an extra RTT_MS
        (the TCP/TLS handshake that persistent connections avoid).
        """
        path = self.path.split("?")[0]
        if RTT_MS > 0 and path.startswith("/rtt/"):
            delay = RTT_MS if not self.first_request else 2 * RTT_MS
            time.sleep(delay / 1000.0)
        self.first_request = False
        with _stats_lock:
            _stats["requests"] += 1

    def do_GET(self):
        if self.path.split("?")[0] == "/__stats":
            with _stats_lock:
                body = json.dumps(_stats).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        self._maybe_rtt()
        f = self._resolve()
        if not os.path.isfile(f):
            body = b"not found"
            self.send_response(404)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        size = os.path.getsize(f)
        rng = self.headers.get("Range")
        if rng:
            m = re.match(r"bytes=(\d*)-(\d*)", rng)
            start = int(m.group(1) or 0) if m else 0
            end = int(m.group(2)) if m and m.group(2) else size - 1
            end = min(end, size - 1)
            if start > end:
                self.send_response(416)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            with open(f, "rb") as fh:
                fh.seek(start)
                data = fh.read(end - start + 1)
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("ETag", '"e2e"')
            self.send_header("Content-Type", self.guess(f))
            self.end_headers()
            self.wfile.write(data)
        else:
            with open(f, "rb") as fh:
                data = fh.read()
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("ETag", '"e2e"')
            self.send_header("Content-Type", self.guess(f))
            self.end_headers()
            self.wfile.write(data)

    def guess(self, f):
        if f.endswith(".m3u8"):
            return "application/vnd.apple.mpegurl"
        if f.endswith(".ts"):
            return "video/mp2t"
        if f.endswith(".html"):
            return "text/html"
        return "application/octet-stream"


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


if __name__ == "__main__":
    os.chdir(ROOT)
    srv = Server(("127.0.0.1", PORT), Handler)
    print(f"serving {ROOT} on 127.0.0.1:{PORT}", flush=True)
    srv.serve_forever()
