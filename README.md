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

## Usage

Querying the dictionary returns lightweight borrowed entry views. These views expose
field-named accessors, serialize directly with Serde, and can be converted to owned
`LexicalUnit` values with `to_owned()` when longer-lived storage is needed.
Paired Wiktionary examples expose optional `simplified`, `traditional`, and
`english` fields; the borrowed `ExampleRef` provides matching accessors without
materializing strings.

Each `LexicalUnitRef` also exposes `commonness()`. A score of `0.0` means the
identity was unseen in the configured frequency corpora; commonness is ranking
evidence and must not be used to filter rare vocabulary.

Chinese and Pinyin searches preserve token-span order and sort the results inside
each span by descending commonness. English searches preserve the existing match
quality ranking and use commonness only to break otherwise equal evidence ranks.
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
selected concept in query order. Within a concept, whole-text, completion,
derivation, omission, surrounding-text, and transformation evidence retain their
existing precedence; commonness breaks ties before the stable identity fallback.

Applications that need concept byte ranges and match evidence can use the separate
`search_english` API. Its overall and per-concept limits must be in `1..=200`.
Completion for an unfinished final token is enabled by default and can be disabled
through `EnglishSearchOptions`. `discovery_truncated` reports only that a search
work budget prevented complete discovery; reaching an output limit does not set it.

See [`examples/borrowed_to_json.rs`](examples/borrowed_to_json.rs) for direct JSON
serialization and [`docs/performance.md`](docs/performance.md) for reproducible
benchmark workloads. The later Syng application update is summarized in
[`docs/migration-v4.md`](docs/migration-v4.md).

## License and data attribution

Library source is licensed under the [MIT License](LICENSE). Bundled dictionary data
is licensed and attributed separately in
[`data/LICENSE-DATA.txt`](data/LICENSE-DATA.txt),
[`data/LICENSE-WORDNET.txt`](data/LICENSE-WORDNET.txt),
[`data/NOTICE.md`](data/NOTICE.md), and
[`data/wiktionary-attribution.json`](data/wiktionary-attribution.json).
