# Syng migration guide for chinese_dictionary 4.0

Syng is intentionally not changed in this release pass. Its later migration
should update the native dependency to `chinese_dictionary = "4.0.0"` and make
the following API changes together.

- `query` now returns `Option<Vec<LexicalUnitRef<'static>>>`; every
  `query_by_*` function returns `Vec<LexicalUnitRef<'static>>`.
- Replace direct lexical field access with view accessors such as
  `entry.id()`, `entry.simplified()`, `entry.pinyin().numbers()`,
  `entry.hsk()`, and `entry.english()`.
- Return borrowed views directly from Tauri commands when they are immediately
  serialized. `LexicalUnitRef` and every nested view implement Serde without
  materializing the owned lexical model.
- Call `entry.to_owned()` only at a boundary that truly needs an owned
  `LexicalUnit` beyond the archive's static lifetime.
- Replace `LexicalId::as_str()` with `Display`/`to_string()` for stored text, or
  `format_into(&mut [u8; 66])` when allocation-free formatting matters.
- Parse database identity strings with `LexicalId::from_str`, then use
  `query_by_id(&id)` for the allocation-free constant-time lookup. New IDs can
  be constructed before the word exists in the corpus with `LexicalId::new`.
- Remove use of English cursors, continuation pages, `has_more`, and runtime
  keys. Structured `EnglishHit::entry_index` addresses the corresponding item
  in `EnglishSearchResult::entries`.
- English limits must be in `1..=200`; the defaults are 50 overall and 20 per
  selected concept. Chinese and Pinyin result counts are unchanged.

Persist `LexicalId`, never the private runtime vector index. The digest and
external `1:<hex>` representation remain identity version 1 and preserve the
existing test vectors.
