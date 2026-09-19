# Change Log
All notable changes to this project will be documented in this file. This project adheres to [Semantic Versioning](http://semver.org/).

## [4.1.0] - 2026-09-19
### Breaking
- Published-crate builds and deployments now require the version-matched
  `chinese_dictionary-data-4.1.0` release bundle through
  `CHINESE_DICTIONARY_DATA_DIR`. This is a breaking build/deployment change
  despite retaining the 4.x version; builds never download the data.

### Changed
- Reduced the crates.io package size by moving the immutable production data to
  a versioned GitHub Release bundle.
- Added strict version, file-name, length, checksum, schema, archive, and search
  index validation for supplied bundles.
- Added a permanent `wiktionary-attribution-4.1.0.json` release asset for
  attribution and redistribution independently of the build bundle.
- Added small generated fixtures for docs.rs builds so API documentation can be
  built without downloading the production data bundle.

### Fixed
- Forced LF line endings for checksummed repository data and license files so
  Windows Git checkouts do not invalidate the trusted bundle checksums.

## [4.0.0] - 2026-09-19
### Breaking
- Replaced the public `WordEntry` model with the structured schema-4 `LexicalUnit` model and persistent `LexicalId` values.
- Lookup functions now return borrowed `LexicalUnitRef` views with accessor methods; call `to_owned()` when an owned model is required.
- Replaced exact English term lookup with bounded lexical concept search. English search limits are now bounded to `1..=200`, with defaults of 50 results overall and 20 results per selected concept.

### Added
- A validated zero-copy lexical archive with constant-time digest identity lookup and native FST indexes.
- Phrase, containment, morphology, optional-grammar, spelling-alias, and final-token English search.
- Structured concept results with per-match details and work-limit reporting.
- Direct Serde serialization and explicit owned conversion for borrowed lexical views.
- Self-contained schema types and English-search format support in the published crate.
- Structured example sentences with optional simplified and traditional fields for Wiktionary examples.
- Structured `LexicalKind::Idiom` metadata from reviewed CC-CEDICT labels.
- A document-normalized `commonness` score on every lexical unit, exposed
  without allocation through `LexicalUnitRef::commonness()`.

### Changed
- Chinese and Pinyin results are ordered by descending commonness within each
  query span while preserving span order.
- Refreshed the bundled schema-4 corpus with source-priority pronunciation handling,
  Chinese Notes enrichment-only metadata, CC-CEDICT semicolon definitions, and
  deterministic Wiktionary example pairing and generated missing-script example
  fallbacks.

## [3.0.0] - 2026-09-08
### Breaking
- Updated `WordEntry.hsk` from a single numeric level to the HSK 2015, Proficiency Standard 2021, and HSK Exam Syllabus 2025 structures.
- Updated embedded dictionary data to the `syng-dictionary-creator` 3.0.0 format.

### Added
- Public `HskLevel` and `HskLevels` types.

## [2.1.8] - 2026-08-16
### Fixed
- Normalized whitespace and sentence punctuation before classifying and searching English and Pinyin queries
- Supported straight and curly apostrophes in Pinyin queries by using the existing joined index keys

## [2.1.7] - 2026-08-16
### Fixed
- Built Chinese tokenization from the dictionary headwords and searched both script indexes to prevent incomplete lookup results

### Changed
- Updated dictionary data

## [2.1.4] - 2023-06-03
### Changed
- Updated dictionary data

## [2.1.3] - 2023-03-27
### Changed
- Updated dictionary and dependencies

## [2.1.2] - 2022-10-12
### Changed
- Updated dependencies
- Derived `Serialize` trait on structs

## [2.1.1] - 2022-09-10
### Fixed
- Fixed various bugs in search when querying with capitalization, empty string, and space

## [2.1.0] - 2022-07-26
### Added
- Added ability to make an exact query by traditional or simplified as described in #8

### Changed
- Performance improvements as described in #8

## [2.0.0] - 2022-07-07
### Changed
- Performance improvements

## [1.0.2] - 2022-05-07
### Changed
- Updated dependencies

## [1.0.1] - 2021-01-20
### Fixed
- Fixed an issue with longer chinese character queries

## [1.0.0] - 2021-01-01
Initial Stable

## [0.1.0] - 2020-11-28
Initial Release
