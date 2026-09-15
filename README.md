# chinese_dictionary

A searchable Chinese/English dictionary with structured lexical data.

## Features

- Search simplified Chinese, traditional Chinese, Pinyin, or English.
- English lexical search recognizes full phrases, phrases inside longer glosses,
  optional articles and infinitive/copular wrappers, noun and verb inflections,
  spelling aliases, and final-token completion.
- Return structured definitions with source attribution, qualifiers, examples,
  lexical categories, pronunciation variants, classifiers, and HSK memberships.
- Use persistent `LexicalId` values for application storage.
- Convert between traditional and simplified Chinese and tokenize Chinese text.

## Basic lookup

```rust
use chinese_dictionary::query;

let results = query("to run").unwrap();
assert!(!results.is_empty());
println!("{}: {}", results[0].simplified, results[0].english[0].gloss.value);
```

`query_by_english`, `query_by_pinyin`, `query_by_simplified`,
`query_by_traditional`, and `query_by_chinese` select a lookup path explicitly.
All lookup functions return canonical `LexicalUnit` references.

## Structured English search

```rust
use chinese_dictionary::{search_english, EnglishSearchOptions};

let result = search_english("hello my name is", EnglishSearchOptions::default())?;
for concept in &result.concepts {
    println!("query bytes {:?}: {} hits", concept.query_bytes, concept.hits.len());
}
# Ok::<(), chinese_dictionary::EnglishSearchError>(())
```

The structured API returns selected query concepts, their exact byte ranges,
ranked match evidence, a deduplicated flat entry list, continuation cursors, and
flags indicating whether more known results or undiscovered budget-limited
matches may remain. Completion is enabled for an unfinished final token by
default and can be disabled through `EnglishSearchOptions`.

The compatibility `query_by_english` wrapper uses the default limits of 20 hits
per selected concept and 100 unique lexical units overall. Use `search_english`
and `continue_english` when an application needs evidence or pagination.

## Bundle and build behavior

The checked-in schema-5 bundle stores dictionaries and the English search index
as Zstandard archives. `build.rs` verifies every manifest checksum, decompresses
the archives into Cargo's `OUT_DIR`, validates their schema and cross-references,
and then compiles those validated bytes into the library.

The bundle includes its CC BY-SA data license, WordNet license, source notices,
manifest, and Wiktionary attribution.

The checked-in data directory is 73,269,161 bytes for the pinned 2026-09-15
bundle, versus 182,699,416 bytes for the equivalent uncompressed schema-5
artifacts. Build-time decompression does not reduce the final executable: the
release benchmark executable is about 213 MiB because it embeds the unpacked
dictionaries and English index.

## Performance baseline

Run `cargo run --release --example english_search_bench` to measure the full
embedded corpus. On an Apple M4 Max, the initial implementation measured 634 ms
initialization and these warm p95 times: 12 µs for `watermelon`, 241 µs for
`run`, 1.16 ms for `wat`, 6.55 ms for `the`, 31.8 ms for `to be happy`, and
32.7 ms for `hello my name is`.

Exact, morphology, and bounded prefix paths meet the initial target. Broad
common-word retrieval and positional multi-concept discovery do not yet meet the
2 ms p95 target; their deterministic work limits and `discovery_truncated`
reporting keep the behavior bounded while those paths are optimized further.

## Compatibility

Version 4 replaces the legacy `WordEntry` model with `LexicalUnit`. Notable field
changes include:

- `word_id` becomes persistent `id: LexicalId`.
- Pinyin fields move under `pinyin`.
- English strings become structured `Definition` values; display text is in
  `definition.gloss.value`.
- Classifiers become sourced `LexicalId` references.

Runtime `u32` keys are private to a particular bundle. Persist `LexicalId` and
resolve it with `query_by_id` or `query_by_id_str`.

## Bundle compatibility

The schema-5 model and English-search reader are private modules in this crate.
Consequently, a published `chinese_dictionary` package has no dependency on the
generator repository. When the generator changes either binary format, its
schema or format version must be bumped and the matching consumer modules and
bundle must be updated together.

## License

Library source is licensed under the MIT License. The bundled dictionary data is
licensed and attributed separately in `data/LICENSE-DATA.txt`,
`data/LICENSE-WORDNET.txt`, `data/NOTICE.md`, `data/manifest.json`, and
`data/wiktionary-attribution.json`.
