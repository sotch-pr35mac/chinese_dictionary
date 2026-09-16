# Change Log
All notable changes to this project will be documented in this file. This project adheres to [Semantic Versioning](http://semver.org/).

## [4.0.0] - 2026-09-15
### Breaking
- Replaced `WordEntry` with the structured schema-4 `LexicalUnit` model and persistent `LexicalId` values.
- Replaced the legacy exact English dictionary with bounded lexical concept search.
- Lookup functions now return borrowed `LexicalUnitRef` views; call `to_owned()` when an owned model is required.
- Removed English continuation cursors and pagination. English output limits are now restricted to `1..=200`.

### Added
- A validated zero-copy lexical archive with constant-time digest identity lookup and native FST indexes.
- Phrase, containment, morphology, optional-grammar, spelling-alias, and final-token English search.
- Structured concept results, match evidence, and work-limit reporting.
- Direct Serde serialization and explicit owned conversion for borrowed lexical views.
- Consumer-local schema types and English-search format support, so the published crate has no unpublished path dependencies.

### Changed
- English lookup defaults to 50 unique entries overall and 20 hits per selected concept.
- Stable identities store fixed-size SHA-256 digests while preserving the external `1:<hex>` representation.

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
