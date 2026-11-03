#!/usr/bin/env python3
"""A stand-in for a SearXNG on this device, for v4 D's checks when a real one is not run: it answers
/search?q=...&format=json in SearXNG's JSON form, with results that point at a real public page.
usage: fake_searxng.py PORT"""
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

RESULTS = [
    {"title": "Mango - Wikipedia", "url": "https://en.wikipedia.org/wiki/Mango", "content": "A mango is an edible stone fruit produced by the tropical tree Mangifera indica.", "engine": "stand-in"},
    {"title": "Mangifera indica - Wikipedia", "url": "https://en.wikipedia.org/wiki/Mangifera_indica", "content": "Mangifera indica, commonly known as mango, is a species of flowering plant.", "engine": "stand-in"},
]


class Search(BaseHTTPRequestHandler):
    def do_GET(self):
        asked = urlparse(self.path)
        query = parse_qs(asked.query)
        ok = asked.path == "/search" and query.get("format") == ["json"] and query.get("q")
        body = json.dumps({"query": query.get("q", [""])[0], "results": RESULTS if ok else []}).encode()
        self.send_response(200 if ok else 403)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


HTTPServer(("127.0.0.1", int(sys.argv[1])), Search).serve_forever()
