#!/usr/bin/env python3
"""Minimal RouterOS v7 REST mock for tmon development (quickstart V9).

Serves GET /rest/ip/dhcp-server/lease and GET /rest/ip/dns/cache from a JSON
file, protected by HTTP basic auth. Standard library only.

    python3 scripts/mock-routeros.py --port 8728 --user tmon-ro --password secret
"""
import argparse
import base64
import hmac
import json
import pathlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROUTES = {"/rest/ip/dhcp-server/lease": "leases", "/rest/ip/dns/cache": "dns"}


def make_handler(data_file: pathlib.Path, user: str, password: str):
    expected = "Basic " + base64.b64encode(f"{user}:{password}".encode()).decode()

    class Handler(BaseHTTPRequestHandler):
        def _send(self, code: int, body: object) -> None:
            payload = json.dumps(body).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def do_GET(self) -> None:  # noqa: N802 (http.server API)
            auth = self.headers.get("Authorization", "")
            if not hmac.compare_digest(auth, expected):
                self.send_response(401)
                self.send_header("WWW-Authenticate", 'Basic realm="routeros"')
                self.end_headers()
                return
            key = ROUTES.get(self.path.split("?", 1)[0])
            if key is None:
                self._send(404, {"error": 404, "message": "Not Found"})
                return
            try:
                data = json.loads(data_file.read_text())
            except (OSError, json.JSONDecodeError) as exc:
                self._send(500, {"error": 500, "message": str(exc)})
                return
            self._send(200, data.get(key, []))

        def log_message(self, fmt: str, *args) -> None:
            print(f"mock-routeros: {self.address_string()} {fmt % args}")

    return Handler


def main() -> None:
    here = pathlib.Path(__file__).resolve().parent
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=8728)
    p.add_argument("--user", default="tmon-ro")
    p.add_argument("--password", default="secret")
    p.add_argument("--data", type=pathlib.Path, default=here / "mock-routeros-data.json")
    a = p.parse_args()
    server = ThreadingHTTPServer((a.host, a.port), make_handler(a.data, a.user, a.password))
    print(f"mock RouterOS REST on http://{a.host}:{server.server_address[1]} (user {a.user})")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
