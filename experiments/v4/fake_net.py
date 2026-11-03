#!/usr/bin/env python3
"""A stand-in for `kestrel-net` beside a scratch copy of kestrel, for v4 C's checks: no mail, no keyring,
no network. It speaks the same JSON lines, checks each approval token the way the real one does, and
writes what it was asked to FAKE_NET_LOG."""
import hashlib
import json
import os
import sys


def approval(key, nonce, mail):
    h = hashlib.sha256()
    for part in (key, nonce, mail["from"], mail["to"], mail["subject"], mail["body"]):
        data = part.encode()
        h.update(len(data).to_bytes(8, "little"))
        h.update(data)
    return h.hexdigest()


def main():
    key = sys.stdin.readline().strip()
    print('{"ready":true}', flush=True)
    log = open(os.environ["FAKE_NET_LOG"], "a")
    for line in sys.stdin:
        request = json.loads(line)
        op = request["op"]
        reply = {"id": request["id"]}
        if op == "smtp_send":
            good = request["approval"] == approval(key, request["nonce"], request["mail"])
            log.write(json.dumps({"op": op, "token_good": good, "mail": request["mail"]}) + "\n")
            reply.update({"sent": True} if good else {"error": "this mail has no approval"})
        elif op == "secret_set":
            log.write(json.dumps({"op": op, "value": request["value"]}) + "\n")
            reply["saved"] = True
        elif op == "secret_delete":
            log.write(json.dumps({"op": op}) + "\n")
            reply["deleted"] = True
        else:
            log.write(json.dumps({"op": op}) + "\n")
            reply["error"] = "not in this stand-in"
        log.flush()
        print(json.dumps(reply), flush=True)


main()
