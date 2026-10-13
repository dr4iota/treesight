#!/bin/bash
# A/B snapshot harness: capture every kind of response the server renders,
# normalized, so a refactor can prove itself byte-identical.
#
#   scripts/snap.sh /tmp/snapwork base     # before the change
#   scripts/snap.sh /tmp/snapwork new      # after
#   diff -r /tmp/snapwork/base /tmp/snapwork/new
#
# The fixture is created once under <workdir>/fixture and reused, so mtimes
# in listings stay stable between the two captures. Delete the workdir to
# start a fresh comparison; never compare captures from different fixtures.
set -eu
WORK="$1"; OUT="$WORK/$2"
FX="$WORK/fixture"
mkdir -p "$WORK"
if [ ! -f "$FX/.done" ]; then
  mkdir -p "$FX/sub"
  printf '# Hello\n\nSome *markdown* with `code`.\n\n```rust\nfn x() {}\n```\n' > "$FX/README.md"
  printf 'fn main() {\n    println!("hi");\n}\n' > "$FX/a.rs"
  printf 'plain text\n' > "$FX/sub/b.txt"
  printf '## Sub doc\n' > "$FX/sub/c.md"
  printf '\x89PNG\r\n\x1a\n0000' > "$FX/img.png"
  printf 'AB\x00CD\x00EF' > "$FX/bin.dat"
  printf 'secret\n' > "$FX/.hidden"
  ln -sfn /etc/hostname "$FX/esc"
  printf 'name,qty,note\r\npear,3,"ripe, soft"\r\nfig,12,"said ""hi"""\r\n' > "$FX/data.csv"
  printf 'a\tb\n1\t<b>\n' > "$FX/data.tsv"
  printf '{"b":1,"a":[1,2.50,{}],"s":"x\\"y"}' > "$FX/min.json"
  printf '{"n":1}\n{"n":2}\n' > "$FX/rows.jsonl"
  printf 'line one\nline two\n' | gzip -n > "$FX/log.txt.gz"
  printf 'k,v\n1,2\n' | gzip -n > "$FX/data.csv.gz"
  touch "$FX/.done"
fi
rm -rf "$OUT"; mkdir -p "$OUT"
cargo build --example snapshot_server
PORTS="$WORK/ports.txt"
cargo run -q --example snapshot_server -- "$FX" > "$PORTS" &
PID=$!
trap 'kill $PID 2>/dev/null' EXIT
for _ in $(seq 50); do grep -q PORTS "$PORTS" 2>/dev/null && break; sleep 0.1; done
read -r _ P1 P2 < "$PORTS"
norm() { sed '/^[Dd]ate:/d' | tr -d '\r'; }
g()  { curl -s  -H 'Accept: text/html' "http://127.0.0.1:$1$2" | norm; }
gi() { curl -si -H 'Accept: text/html' "http://127.0.0.1:$1$2" | norm; }
# CLI-shaped server
g  $P1 '/'                  > "$OUT/root.html"
curl -s "http://127.0.0.1:$P1/" | norm > "$OUT/root.txt"
g  $P1 '/sub/'              > "$OUT/sub.html"
gi $P1 '/sub'               > "$OUT/sub-redirect"
g  $P1 '/a.rs'              > "$OUT/a-rs.html"
g  $P1 '/README.md'         > "$OUT/readme.html"
g  $P1 '/README.md?src=1'   > "$OUT/readme-src.html"
g  $P1 '/a.rs?raw=1'        > "$OUT/a-rs-framed.html"
gi $P1 '/a.rs?raw=1&bare=1' > "$OUT/a-rs-bare"
gi $P1 '/a.rs?dl=1'         > "$OUT/a-rs-dl"
g  $P1 '/img.png'           > "$OUT/img-page.html"
curl -si -H 'Sec-Fetch-Dest: image' "http://127.0.0.1:$P1/img.png" | norm > "$OUT/img-raw"
g  $P1 '/bin.dat'           > "$OUT/bin-page.html"
g  $P1 '/data.csv'          > "$OUT/csv.html"
g  $P1 '/data.csv?src=1'    > "$OUT/csv-src.html"
g  $P1 '/data.tsv'          > "$OUT/tsv.html"
g  $P1 '/min.json'          > "$OUT/json.html"
g  $P1 '/min.json?src=1'    > "$OUT/json-src.html"
g  $P1 '/rows.jsonl'        > "$OUT/jsonl.html"
g  $P1 '/log.txt.gz'        > "$OUT/gz.html"
g  $P1 '/data.csv.gz'       > "$OUT/csv-gz.html"
g  $P1 '/data.csv?raw=1'    > "$OUT/csv-framed.html"
g  $P1 '/?q=%2A.rs'        > "$OUT/search.html"
g  $P1 '/?q=%2A.md&r=1'     > "$OUT/search-rec.html"
gi $P1 '/missing'           > "$OUT/missing"
gi $P1 '/esc'               > "$OUT/esc-403"
g  $P1 '/.hidden'           > "$OUT/hidden.html"
gi $P1 '/.ts/app.css'       > "$OUT/app-css"
curl -si -H 'Range: bytes=2-5' "http://127.0.0.1:$P1/a.rs?raw=1&bare=1" | norm > "$OUT/range"
curl -si -H 'Range: bytes=999999-' "http://127.0.0.1:$P1/a.rs?raw=1&bare=1" | norm > "$OUT/range-416"
# app-shaped server (app_ui + Places/Recent)
g $P2 '/'                        > "$OUT/app-root.html"
g $P2 '/sub/'                    > "$OUT/app-sub.html"
g $P2 '/a.rs'                    > "$OUT/app-a-rs.html"
g $P2 '/.ts/wait?path=/tmp/x'    > "$OUT/app-wait.html"
echo "captured $(ls "$OUT" | wc -l) responses in $OUT"
