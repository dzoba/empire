#!/usr/bin/env python3
"""Serve web/ for local play, telling browsers to always check for a newer build.

Plain `python3 -m http.server` lets browsers reuse cached copies of the game
code without asking, so a rebuild can appear to change nothing.
"""
import functools
import http.server
import os
import sys


class NoCacheHandler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else int(os.environ.get("PORT", "8080"))
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "web")
    handler = functools.partial(NoCacheHandler, directory=os.path.normpath(root))
    with http.server.ThreadingHTTPServer(("127.0.0.1", port), handler) as httpd:
        print(f"Serving Empire on http://localhost:{port}", flush=True)
        httpd.serve_forever()


if __name__ == "__main__":
    main()
