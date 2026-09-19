# chinese_dictionary

## About

A searchable Chinese/English dictionary with structured lexical data.

## Features

- Search simplified Chinese, traditional Chinese, Pinyin, or English.
- Classify text as Chinese, Pinyin, English, or uncertain.
- Match English phrases, morphology, spelling aliases, and optional final-token completion.
- Expose a document-normalized commonness score for ranking ambiguous results.
- Return structured definitions, qualifiers, examples, pronunciation variants, classifiers, HSK memberships, and source attribution.
- Convert between traditional and simplified Chinese and tokenize Chinese text.

## Installation

Add the library to your application:

```toml
[dependencies]
chinese_dictionary = "4.1.0"
```

Version 4.1.0 requires a version-matched external data bundle before Cargo can
compile the crate. Builds do not download data. Download the bundle and checksum
file from the v4.1.0 release, verify the bundle, and extract it:

```sh
mkdir -p vendor/chinese_dictionary-data-4.1.0
curl -fLO https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/chinese_dictionary-data-4.1.0.tar.gz
curl -fLO https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/SHA256SUMS
grep ' chinese_dictionary-data-4.1.0.tar.gz$' SHA256SUMS | shasum -a 256 -c -
tar -xzf chinese_dictionary-data-4.1.0.tar.gz -C vendor/chinese_dictionary-data-4.1.0
```

Set the directory for one shell:

```sh
export CHINESE_DICTIONARY_DATA_DIR="$PWD/vendor/chinese_dictionary-data-4.1.0"
cargo build
```

For a persistent project configuration, create `.cargo/config.toml`:

```toml
[env]
CHINESE_DICTIONARY_DATA_DIR = { value = "vendor/chinese_dictionary-data-4.1.0", relative = true }
```

Cargo resolves a `relative = true` value against the directory containing the
configuration file and passes an absolute path to the build script. The build
never accesses the network: it validates the version, names, lengths, checksums,
and internal structure of the supplied bundle before compiling. A mismatched,
incomplete, or modified bundle fails validation.

The data archive is the only release asset required to compile. It already
contains `wiktionary-attribution.json`, the data notices, and the data licenses.
The standalone
[`wiktionary-attribution-4.1.0.json`](https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/wiktionary-attribution-4.1.0.json)
asset is provided for attribution and redistribution workflows; it is not a
second build input.

Repository contributors do not need this setup step. The repository tracks the
reviewed release data and its `.cargo/config.toml` points builds at `data/`.
Cargo's package allowlist excludes those large files from the crates.io archive.

In CI, cache or download the same immutable bundle, extract it into the
workspace, and set `CHINESE_DICTIONARY_DATA_DIR` for build and test steps. For
example:

```yaml
- name: Provision chinese_dictionary data
  shell: bash
  run: |
    mkdir -p vendor/chinese_dictionary-data-4.1.0
    curl -fLO https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/chinese_dictionary-data-4.1.0.tar.gz
    curl -fLO https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/SHA256SUMS
    grep ' chinese_dictionary-data-4.1.0.tar.gz$' SHA256SUMS | shasum -a 256 -c -
    tar -xzf chinese_dictionary-data-4.1.0.tar.gz -C vendor/chinese_dictionary-data-4.1.0
- run: cargo test
  env:
    CHINESE_DICTIONARY_DATA_DIR: ${{ github.workspace }}/vendor/chinese_dictionary-data-4.1.0
```

For offline builds, vendor the extracted directory alongside your application
and commit or otherwise mirror it in your internal dependency store. Keep the
same `.cargo/config.toml` entry; no network access is attempted during a build.

Upgrading from 3.0.0? Read the complete
[`3.0.0` to `4.1.0` migration guide](docs/migration-v4.md), including the API,
model, search, serialization, identity, and deployment changes.

## Usage

Querying the dictionary returns lightweight borrowed entry views. These views expose
field-named accessors, serialize directly with Serde, and can be converted to owned
`LexicalUnit` values with `to_owned()` when longer-lived storage is needed.
Paired Wiktionary examples expose optional `simplified`, `traditional`, and
`english` fields; the borrowed `ExampleRef` provides matching accessors without
materializing strings.

Each `LexicalUnitRef` also exposes `commonness()`. A score of `0.0` means the
identity was unseen in the configured frequency corpora; commonness should be
used as ranking evidence and isn’t necessarily authoritative of the word’s
overall usage frequency given the size and limits of the corpora.

Chinese and Pinyin searches preserve token-span order and sort the results inside
each span by descending commonness. English searches rank the most direct matches
first. Matches that require final-token completion, alternate spellings,
morphology, omitted parenthetical text, surrounding text, or other transformations
rank lower. Commonness breaks ties between otherwise equally direct matches.
English result groups follow their selected concepts in query order rather than
being interleaved across concepts.

```rust
use chinese_dictionary::query;

let results = query("to run").unwrap();
assert!(results.iter().any(|entry| entry.simplified() == "执行"));
```

Use a language-specific function when the language is already known:
`query_by_english`, `query_by_pinyin`, `query_by_simplified`,
`query_by_traditional`, or `query_by_chinese`. `query_by_id` and
`query_by_id_str` resolve a persistent lexical identity directly.

Classifying a string returns one of the following `ClassificationResult` values:

- `PY` — Pinyin
- `EN` — English
- `ZH` — Chinese
- `UN` — uncertain

```rust
use chinese_dictionary::{classify, ClassificationResult};

assert_eq!(ClassificationResult::PY, classify("nihao"));
assert_eq!(ClassificationResult::ZH, classify("你好"));
```

Traditional and simplified conversion and dictionary-driven tokenization remain
available independently of lookup:

```rust
use chinese_dictionary::{simplified_to_traditional, tokenize, traditional_to_simplified};

assert_eq!("简体字", traditional_to_simplified("簡體字"));
assert_eq!("繁體字", simplified_to_traditional("繁体字"));
assert_eq!(vec!["今天", "天气", "不错"], tokenize("今天天气不错"));
```

## English search

`query_by_english` uses the same borrowed entry-list result type as the other
language-specific functions. It returns at most 50 unique entries by default,
with at most 20 hits contributed by one selected concept. Results are grouped by
selected concept in query order. Within a concept, more direct English matches
rank first; matches that require final-token completion, alternate spellings,
morphology, or other transformations rank lower. Commonness breaks ties between
otherwise equally direct matches.

Applications that need concept byte ranges and per-match details can use the
separate `search_english` API. Its overall and per-concept limits must be in
`1..=200`.
Completion for an unfinished final token is enabled by default and can be disabled
through `EnglishSearchOptions`. `discovery_truncated` reports only that a search
work budget prevented complete discovery; reaching an output limit does not set it.

See [`examples/borrowed_to_json.rs`](examples/borrowed_to_json.rs) for direct JSON
serialization.

## License and data attribution

Library source is licensed under the [MIT License](LICENSE). Dictionary data is
licensed and attributed separately in
[`data/LICENSE-DATA.txt`](data/LICENSE-DATA.txt),
[`data/LICENSE-WORDNET.txt`](data/LICENSE-WORDNET.txt), and
[`data/NOTICE.md`](data/NOTICE.md). Per-entry English Wiktionary attribution is
available in the permanent
[`wiktionary-attribution-4.1.0.json`](https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/wiktionary-attribution-4.1.0.json)
release asset.

Applications that redistribute compiled dictionary data must retain the data
license, WordNet license, notices, and the version-specific attribution link
supplied with this bundle. Do not imply endorsement by upstream projects or
contributors.
