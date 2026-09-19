# Migrating from chinese_dictionary 3.0.0 to 4.1.0

This guide describes the public API and data-model changes from the 3.0.0
release to 4.1.0. It is cumulative: the API and model changes landed in 4.0.0,
and 4.1.0 adds a required build and deployment step for the external data
bundle.

## Installation and deployment in 4.1.0

Although the Rust API remains compatible with 4.0.0, moving the production data
out of the crates.io package is a breaking build and deployment change. Add the
4.1.0 dependency and provision the exact matching bundle before compiling:

```toml
[dependencies]
chinese_dictionary = "4.1.0"
```

```sh
mkdir -p vendor/chinese_dictionary-data-4.1.0
curl -fLO https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/chinese_dictionary-data-4.1.0.tar.gz
curl -fLO https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/SHA256SUMS
grep ' chinese_dictionary-data-4.1.0.tar.gz$' SHA256SUMS | shasum -a 256 -c -
tar -xzf chinese_dictionary-data-4.1.0.tar.gz -C vendor/chinese_dictionary-data-4.1.0
export CHINESE_DICTIONARY_DATA_DIR="$PWD/vendor/chinese_dictionary-data-4.1.0"
cargo build
```

The build script never downloads data. It validates the bundle version, required
file names, byte lengths, checksums, schema and archive metadata, and search
index structure. A bundle from another crate version, an incomplete extraction,
or any modified file fails the build.

To persist the path for the project, create `.cargo/config.toml`:

```toml
[env]
CHINESE_DICTIONARY_DATA_DIR = { value = "vendor/chinese_dictionary-data-4.1.0", relative = true }
```

Cargo resolves that relative path from the directory containing the
configuration file. Repository checkouts already use this mechanism to point at
the reviewed `data/` directory; the large production files are not included in
the published crate.

CI must provision the same immutable bundle before any build, test, or doctest
step. For example:

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

For offline builds, vendor the extracted directory with the application or
mirror it in an internal dependency store, then keep the persistent Cargo
configuration above. Cargo and the build script make no network request.

The bundle already contains `wiktionary-attribution.json`, `NOTICE.md`, and the
data license files required for redistribution. The separate
[`wiktionary-attribution-4.1.0.json`](https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/wiktionary-attribution-4.1.0.json)
release asset is a convenient permanent attribution resource, not an additional
input needed to compile.

## API and data model

- Replace `WordEntry` with the structured `LexicalUnit` model and persistent
  `LexicalId` values.
- `query` now returns `Option<Vec<LexicalUnitRef<'static>>>`, and each
  `query_by_*` function returns `Vec<LexicalUnitRef<'static>>`.
- Replace direct lexical field access with view accessors such as `entry.id()`,
  `entry.simplified()`, `entry.pinyin().numbers()`, `entry.commonness()`,
  `entry.hsk()`, and `entry.english()`.
- Call `entry.to_owned()` at a boundary that truly needs an owned `LexicalUnit`
  beyond the archive's static lifetime.
- Replace persisted numeric `word_id` values with `LexicalId`. Parse stored
  identity strings with `LexicalId::from_str`, then use `query_by_id(&id)` for
  lookup. New IDs can be constructed with `LexicalId::new`.
- Use `Display`/`to_string()` for the external identity representation, or
  `format_into(&mut [u8; 66])` when allocation-free formatting matters.

## Search behavior

- Chinese and Pinyin searches preserve token-span order and sort results within
  each span by descending commonness. A zero commonness score does not filter an
  identity.
- English lookup now searches definitions rather than requiring an exact English
  term as in 3.0.0. More direct matches rank first; matches that require
  final-token completion, alternate spellings, morphology, or other
  transformations rank lower. Commonness breaks ties between otherwise equally
  direct matches.
- `query_by_english` returns at most 50 unique entries by default, with at most
  20 hits contributed by one selected concept. The structured `search_english`
  API accepts overall and per-concept limits in `1..=200`.
- English result groups follow selected concepts in query order. Use
  `EnglishHit::entry_index` to address the corresponding item in
  `EnglishSearchResult::entries`.

## Examples and serialization

- Structured example sentences are new in 4.0.0. They expose independent
  optional simplified and traditional text through `ExampleRef::simplified()`
  and `ExampleRef::traditional()`. Paired Wiktionary script variants are
  returned as one example instead of duplicate records.
- Return borrowed views directly from Tauri commands when they are immediately
  serialized. `LexicalUnitRef` and its nested views implement Serde without
  materializing the owned lexical model.

The digest and external `1:<hex>` identity representation remain identity
version 1. Persist `LexicalId`, not an internal archive position.
