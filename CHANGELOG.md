# Change Log
All notable changes to this project will be documented in this file. This project adheres to [Semantic Versioning](http://semver.org/).

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
