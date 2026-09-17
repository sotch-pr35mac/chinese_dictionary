# Migrating from chinese_dictionary 3.0.0 to 4.0.0

This guide describes the public API and data-model changes from the 3.0.0
release to 4.0.0. Syng is intentionally not changed in this release pass; when
Syng adopts `chinese_dictionary = "4.0.0"`, make the following changes together.

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
- English lookup now uses lexical concept search rather than the exact English
  term lookup available in 3.0.0. Matches are ranked by v4 match evidence, with
  commonness breaking otherwise equal evidence ranks.
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
