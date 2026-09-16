# Schema-4 performance results — 2026-09-16

These measurements compare the pre-change commit `15936f5` with the schema-4
archive implementation on Apple Silicon (`arm64`), macOS 26.6.2, using Rust
1.97.1 in the release profile. The corpus is identified by the new manifest
SHA-256 `06132c0b1778b45e2b116795872aec0289cd6596efc00ffebdeec7fc9e5850ff`.

Both search runs used completion disabled, an overall limit of 50, a
per-concept limit of 20, and included JSON serialization. The old path
serialized its borrowed owned-model references; the new path serialized the
structured result directly from archived views.

- Initialization: 586 ms before; 45 ms with cached pages and 229 ms on the
  first observed schema-4 run.
- Maximum resident set size: 746,536,960 bytes before; 223,363,072 bytes after.
- Peak memory footprint: 565,560,280 bytes before; 70,353,472 bytes after.
- Benchmark executable: 223,344,048 bytes before; 194,186,560 bytes after.
- `watermelon` p95: 13.7 µs before; 14.0 µs after.
- `run` p95: 112 µs before; 97 µs after.
- `to be happy` p95: 40.5 ms before; 1.17 ms after.
- `the` p95: 4.13 ms before; 0.89 ms after.
- `hello my name is` p95: 45.4 ms before; 39.1 ms after.

The complex `hello my name is` case still misses the 2 ms p95 target and is
reported as an open performance limitation. Every other measured committed
case met the target. Autocomplete p95 was 1.30 ms for `wat`, 22.2 µs for
`waterm`, and 0.94 ms for `hel` including JSON.

Parsed identity lookup measured 42 ns p95 for both repeated and distributed
IDs. Parsing the fixed external identity measured 42 ns p95, and combined
string parsing plus lookup measured 84 ns p95. The counting allocator confirmed
zero heap allocations for parsed-ID lookup and ID parsing.

Representative allocation counts were:

- committed `to be happy` search: 227,223 allocations / 3,412,938 requested bytes;
- serialization of its borrowed result: 10 allocations / 130,944 requested bytes;
- `hello my name is`: 2,177,523 allocations / 17,438,381 requested bytes; and
- `wat` autocomplete: 66,642 allocations / 572,271 requested bytes.

The archived path constructs no owned `LexicalUnit` in normal lookup or JSON
serialization. The remaining English planner allocations, especially for the
complex multi-concept case, are the clearest follow-up optimization target.

The generated bundle is 67,166,352 bytes, down from 73,273,239 bytes by
6,106,887 bytes. `dictionary.rkyv.zst` is 30,220,898 bytes from a
119,996,128-byte archive and replaces 36,325,612 bytes of dictionary/index
artifacts. Two independent full generator runs were byte-identical.

A separate full-corpus parity program decoded the pre-change schema-5 artifacts
and compared them with public schema-4 lookups. It verified JSON-equivalent
content and identical stable identity for all 205,493 lexical records, plus
ordered association parity for 177,971 simplified keys, 179,151 traditional
keys, and 442,688 Pinyin keys.
