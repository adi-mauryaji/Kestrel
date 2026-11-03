#!/usr/bin/env bash
# CPython 3.14.7 for WASI, the code sandbox's Python (v4 A4, registry `cpython-wasi`): python.wasm and
# its standard library into third_party/python-wasi (not in git), checked against its SHA-256. The
# same Python as v3's sandbox on this laptop, for the parity check. Installers bundle the folder.
set -euo pipefail
cd "$(dirname "$0")/.."

name=python-3.14.7-wasi_sdk-24.zip
url=https://github.com/brettcannon/cpython-wasi-build/releases/download/v3.14.7/$name
sha256=2e064d3fb8172471d39d741348efa722349c40b96301f69968dff714999c584b
out=third_party/python-wasi

if [[ -f $out/python.wasm ]]; then
  echo "$out is there"
  exit 0
fi
mkdir -p third_party
zip=third_party/$name
curl -fsSL --retry 5 --retry-all-errors -o "$zip" "$url"
got=$( (shasum -a 256 "$zip" 2>/dev/null || sha256sum "$zip") | cut -d' ' -f1)
if [[ $got != "$sha256" ]]; then
  rm -f "$zip"
  echo "$name: SHA-256 $got, expected $sha256" >&2
  exit 1
fi
rm -rf "$out.new"
mkdir -p "$out.new"
# unzip, or bsdtar where there is none (Windows, macOS).
(cd "$out.new" && { unzip -q "../$name" 2>/dev/null || tar -xf "../$name"; })
mv "$out.new" "$out"
rm -f "$zip"
echo "$out: $(du -sh "$out" | cut -f1)"
