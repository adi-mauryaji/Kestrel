#!/usr/bin/env bash
# Blueprint rule 2: the core links no network client; only kestrel-net does. Fails if `kestrel`'s own
# dependency tree holds one. wasmtime-wasi brings tokio and socket2, known and allowed: the code
# sandbox gives Python only WASI preview 1, which has no sockets, and they are no client.
set -euo pipefail
cd "$(dirname "$0")/.."

found=$(cargo tree -p kestrel-cli --features llama -e normal --prefix none |
  sed 's/ v[0-9].*//' | sort -u |
  grep -xE 'ureq|reqwest|hyper|hyper-.*|rustls|rustls-.*|native-tls|openssl|openssl-.*|lettre|curl|curl-sys|isahc|attohttpc|surf|tungstenite|h2|h3|quinn|webpki.*|kestrel-net|keyring|keyring-core' || true)
if [[ -n $found ]]; then
  echo "the core links a network client:"
  echo "$found"
  exit 1
fi
echo "the core links no network client"
