//! ### About
//! A searchable Chinese / English dictionary with helpful utilities.
//!
//! ### Features
//! - Search with Traditional Chinese characters, Simplified Chinese characters, pinyin with tone marks, pinyin with tone numbers, pinyin with no tones, and English.
//! - Classify a string of text as either English, pinyin, or Chinese characters.
//! - Convert between Traditional and Simplified Chinese characters.
//! - Segment strings of Chinese characters into tokens using a dictionary-driven segmentation approach.
//!
//! ### Usage
//! Querying the dictionary
//! ```rust
//! extern crate chinese_dictionary;
//!
//! use chinese_dictionary::query;
//!
//! // Querying the dictionary returns zero-copy `LexicalUnitRef` views.
//! let text = "to run";
//! let results = query(text).unwrap();
//! assert!(results.iter().any(|entry| entry.simplified() == "执行"));
//! ```
//!
//! Classifying a string of text
//! ```rust
//! extern crate chinese_dictionary;
//!
//! use chinese_dictionary::{classify, ClassificationResult};
//!
//! // Read more about the ClassificationResult enum below
//! assert_eq!(ClassificationResult::PY, classify("nihao"));
//! ```
//!
//! Convert between Traditional and Simplified Chinese characters
//! ```rust
//! extern crate chinese_dictionary;
//!
//! use chinese_dictionary::{traditional_to_simplified, simplified_to_traditional};
//!
//! assert_eq!("简体字", traditional_to_simplified("簡體字"));
//! assert_eq!("繁體字", simplified_to_traditional("繁体字"));
//! ```
//!
//! Segment a string of characters
//! ```rust
//! extern crate chinese_dictionary;
//!
//! use chinese_dictionary::{tokenize};
//!
//! assert_eq!(vec!["今天", "天气", "不错"], tokenize("今天天气不错"));
//! ```
//!
//! #### `LexicalUnitRef`
//! Each result borrows stable identity, headwords, structured Pinyin, HSK data,
//! sourced English definitions, classifiers, and pronunciation variants directly
//! from the embedded archive. Use `to_owned()` only when an owned `LexicalUnit`
//! is needed.
//!
//! #### `ClassificationResult` enum
//! The possible values for the `ClassificationResult` enum are:
//! - `PY`: Represents Pinyin
//! - `EN`: Represents English
//! - `ZH`: Represents Chinese
//! - `UN`: Represents an uncertain classification result

extern crate character_converter;
extern crate chinese_detection;
extern crate once_cell;

mod chinese_dictionary;
mod dictionary_archive;
mod english;
mod english_search_format;
mod model;
pub use self::chinese_dictionary::{
    classify, init, is_simplified, is_traditional, query, query_by_chinese, query_by_english,
    query_by_id, query_by_id_str, query_by_pinyin, query_by_simplified, query_by_traditional,
    simplified_to_traditional, tokenize, traditional_to_simplified, ClassificationResult,
};
pub use self::english::{
    search_english, CompletionMode, EnglishConcept, EnglishHit, EnglishMatchEvidence,
    EnglishMatchKind, EnglishSearchError, EnglishSearchOptions, EnglishSearchResult,
};
pub use self::model::{
    AlternativePronunciation, AlternativePronunciationRef, Definition, DefinitionRef, Example,
    ExampleRef, HskLevel, HskLevels, HskLevelsRef, LexicalId, LexicalKind, LexicalUnit,
    LexicalUnitRef, ModelError, PartOfSpeech, Pinyin, PinyinRef, Qualifier, QualifierCategory,
    QualifierRef, Source, Sourced, SourcedAlternativePronunciationRef, SourcedExampleRef,
    SourcedLexicalIdRef, SourcedLexicalKindRef, SourcedPartOfSpeechRef, SourcedQualifierRef,
    SourcedStringRef,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn word_ids(entries: Vec<LexicalUnitRef<'static>>) -> Vec<LexicalId> {
        entries.into_iter().map(LexicalUnitRef::id).collect()
    }

    fn query_word_ids(raw: &str) -> Option<Vec<LexicalId>> {
        query(raw).map(word_ids)
    }

    fn expected_chinese_ids(headword: &str) -> Vec<LexicalId> {
        let mut seen = HashSet::new();
        let mut entries = query_by_simplified(headword)
            .into_iter()
            .chain(query_by_traditional(headword))
            .filter(|entry| seen.insert(entry.id()))
            .collect::<Vec<_>>();
        entries.sort_unstable_by(|left, right| {
            right
                .commonness()
                .total_cmp(&left.commonness())
                .then_with(|| left.id().cmp(&right.id()))
        });
        entries.into_iter().map(LexicalUnitRef::id).collect()
    }

    fn assert_contains_all(
        actual: Vec<LexicalUnitRef<'static>>,
        expected: Vec<LexicalUnitRef<'static>>,
    ) {
        let actual_ids: HashSet<LexicalId> = actual.into_iter().map(LexicalUnitRef::id).collect();

        for entry in expected {
            assert!(actual_ids.contains(&entry.id()));
        }
    }

    #[test]
    fn test_search_by_english_1() {
        let text = "watermelon";
        let result = query(text);
        let actual = result.unwrap().first().unwrap().traditional();
        let expected = "西瓜";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_english_2() {
        let result = query("to run").unwrap();
        assert!(result.iter().any(|entry| entry.traditional() == "執行"));
    }

    #[test]
    fn test_search_by_english_3() {
        let raw = "people around the world";
        let result = search_english(raw, EnglishSearchOptions::default()).unwrap();
        assert!(!result.entries.is_empty());
        assert!(result
            .concepts
            .iter()
            .any(|concept| concept.query_bytes == (0..raw.len())));
    }

    #[test]
    fn conservative_prefers_the_common_learner_facing_translation() {
        let results = query_by_english("conservative");
        let describe = || {
            results
                .iter()
                .take(20)
                .map(|entry| {
                    format!(
                        "{} ({}, {:.6})",
                        entry.simplified(),
                        entry.pinyin().numbers(),
                        entry.commonness()
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let baoshou = results
            .iter()
            .position(|entry| entry.simplified() == "保守")
            .unwrap_or_else(|| panic!("保守 missing from conservative results: {}", describe()));
        let yuju = results
            .iter()
            .position(|entry| entry.pinyin().numbers() == "yu1ju1")
            .unwrap_or_else(|| panic!("yu1ju1 missing from conservative results: {}", describe()));

        assert!(
            baoshou < yuju,
            "保守 should outrank yu1ju1 for conservative: {}",
            describe()
        );
    }

    #[test]
    fn test_search_by_traditional() {
        let text = "繁體字";
        let result = query(text);
        let actual = result
            .unwrap()
            .first()
            .unwrap()
            .english()
            .next()
            .unwrap()
            .gloss()
            .value();
        let expected = "traditional Chinese character";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_simplified() {
        let text = "龙纹";
        let result = query(text);
        let actual = result
            .unwrap()
            .first()
            .unwrap()
            .english()
            .next()
            .unwrap()
            .gloss()
            .value();
        let expected = "dragon (as a decorative design)";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_simplified_exact() {
        let text = "龙纹";
        let result = query_by_simplified(text);
        let actual = result
            .first()
            .unwrap()
            .english()
            .next()
            .unwrap()
            .gloss()
            .value();
        let expected = "dragon (as a decorative design)";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_traditional_exact() {
        let text = "繁體字";
        let result = query_by_traditional(text);
        let actual = result
            .first()
            .unwrap()
            .english()
            .next()
            .unwrap()
            .gloss()
            .value();
        let expected = "traditional Chinese character";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_sentence() {
        let text = "你好今天的天气还好。";
        let result = query(text);
        let actual = result.unwrap().first().unwrap().simplified();
        let expected = "你好";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_pinyin_1() {
        let text = "hánlěng";
        let result = query(text);
        let actual = result.unwrap().first().unwrap().traditional();
        let expected = "寒冷";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_pinyin_2() {
        let text = "dian4nao3";
        let result = query(text);
        let actual = result.unwrap().first().unwrap().traditional();
        let expected = "電腦";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_pinyin_3() {
        let text = "nihao";
        let result = query(text);
        let actual = result.unwrap().first().unwrap().traditional();
        let expected = "你好";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_tokenize_traditional() {
        let sentence = "今天的天氣挺爽";
        let actual = tokenize(sentence);
        let expected = vec![
            "今天".to_string(),
            "的".to_string(),
            "天氣".to_string(),
            "挺".to_string(),
            "爽".to_string(),
        ];
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_tokenize_simplified() {
        let sentence = "今天的天气挺爽";
        let actual = tokenize(sentence);
        let expected = vec![
            "今天".to_string(),
            "的".to_string(),
            "天气".to_string(),
            "挺".to_string(),
            "爽".to_string(),
        ];
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_tokenize_complex() {
        let sentence = "红色是我favorite颜色。";
        let actual = tokenize(sentence);
        let expected = vec![
            "红色".to_string(),
            "是".to_string(),
            "我".to_string(),
            "颜色".to_string(),
        ];
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_dictionary_headword_tokenization_and_lookup() {
        for headword in [
            "以后",
            "以後",
            "用于",
            "用於",
            "万",
            "萬",
            "舍不得",
            "捨不得",
        ] {
            assert_eq!(vec![headword], tokenize(headword));

            let expected = expected_chinese_ids(headword);
            assert!(
                !expected.is_empty(),
                "Missing exact index fixture: {headword}"
            );
            assert_eq!(expected, word_ids(query_by_chinese(headword)));
        }
    }

    #[test]
    fn test_ambiguous_headword_returns_all_unique_entries() {
        let results = query_by_chinese("万");
        let ids = word_ids(results);
        let unique_ids: HashSet<LexicalId> = ids.iter().cloned().collect();

        assert_eq!(expected_chinese_ids("万"), ids);
        assert!(ids.len() >= 3);
        assert_eq!(ids.len(), unique_ids.len());
    }

    #[test]
    fn test_affected_pinyin_queries_are_unchanged() {
        assert_contains_all(query_by_pinyin("yi3hou4"), query_by_simplified("以后"));
        assert_contains_all(query_by_pinyin("yong4yu2"), query_by_simplified("用于"));
        assert_contains_all(query_by_pinyin("she3bu5de5"), query_by_simplified("舍不得"));
    }

    #[test]
    fn test_english_sentence_punctuation_preserves_results_and_order() {
        let expected = query_word_ids("watermelon");
        let expected_direct = word_ids(query_by_english("watermelon"));

        assert!(matches!(expected.as_ref(), Some(ids) if !ids.is_empty()));
        for variant in [
            "watermelon.",
            "watermelon?",
            "watermelon,",
            "\"watermelon\"",
            "\u{201c}watermelon\u{201d}",
            "watermelon\u{3002}",
            "watermelon\u{ff1f}",
            "watermelon\u{ff0c}",
        ] {
            assert_eq!(
                expected,
                query_word_ids(variant),
                "query variant: {variant}"
            );
            assert_eq!(
                expected_direct,
                word_ids(query_by_english(variant)),
                "English variant: {variant}"
            );
        }
    }

    #[test]
    fn test_internal_punctuation_creates_english_token_boundaries() {
        let raw = "people,around the world";
        let result = search_english(raw, EnglishSearchOptions::default()).unwrap();
        assert!(result.concepts.len() >= 2);
        assert!(result
            .concepts
            .iter()
            .all(|concept| !raw[concept.query_bytes.clone()].contains(',')));
    }

    #[test]
    fn test_pinyin_sentence_punctuation_preserves_results_and_order() {
        for (clean, variants) in [
            (
                "ni hao",
                &["ni hao.", "ni hao?", "ni hao\u{3002}", "ni hao\u{ff1f}"][..],
            ),
            (
                "n\u{01d0} h\u{01ce}o",
                &["n\u{01d0} h\u{01ce}o?", "n\u{01d0} h\u{01ce}o\u{3002}"][..],
            ),
            ("ni3 hao3", &["ni3 hao3.", "ni3 hao3\u{ff1f}"][..]),
            ("l\u{01dc}", &["l\u{01dc}.", "l\u{01dc}\u{3002}"][..]),
        ] {
            let expected = query_word_ids(clean);
            let expected_direct = word_ids(query_by_pinyin(clean));

            assert!(matches!(expected.as_ref(), Some(ids) if !ids.is_empty()));
            for variant in variants {
                assert_eq!(
                    expected,
                    query_word_ids(variant),
                    "query variant: {variant}"
                );
                assert_eq!(
                    expected_direct,
                    word_ids(query_by_pinyin(variant)),
                    "Pinyin variant: {variant}"
                );
            }
        }
    }

    #[test]
    fn test_pinyin_apostrophes_use_existing_joined_index_keys() {
        for (clean, variants) in [
            ("xian", &["Xi'an", "Xi\u{2019}an"][..]),
            ("xi1an1", &["Xi1'an1", "Xi1\u{2019}an1"][..]),
            (
                "x\u{012b}\u{0101}n",
                &["X\u{012b}'\u{0101}n", "X\u{012b}\u{2019}\u{0101}n"][..],
            ),
        ] {
            let expected = query_word_ids(clean);
            let expected_direct = word_ids(query_by_pinyin(clean));

            assert!(matches!(expected.as_ref(), Some(ids) if !ids.is_empty()));
            for variant in variants {
                assert_eq!(
                    expected,
                    query_word_ids(variant),
                    "query variant: {variant}"
                );
                assert_eq!(
                    expected_direct,
                    word_ids(query_by_pinyin(variant)),
                    "Pinyin variant: {variant}"
                );
            }
        }

        assert!(query("Xi'an")
            .unwrap()
            .iter()
            .any(|entry| entry.simplified() == "西安"));
    }

    #[test]
    fn test_pinyin_u_colon_remains_supported() {
        let expected = word_ids(query_by_pinyin("lu:4"));

        assert_eq!(expected, word_ids(query_by_pinyin("lu:4.")));
        assert_eq!(Some(expected), query_word_ids("lu:4."));
        assert_eq!(ClassificationResult::PY, classify("lu:4."));
    }

    #[test]
    fn test_meaningful_english_symbols_remain_supported() {
        let clean = "the lgbt+ community";
        let expected = word_ids(query_by_english(clean));

        assert!(!expected.is_empty());
        assert_eq!(expected, word_ids(query_by_english("the lgbt+ community.")));
        assert_eq!(
            query_word_ids(clean),
            query_word_ids("the lgbt+ community.")
        );
    }

    #[test]
    fn test_punctuation_only_queries_are_empty_or_uncertain() {
        let punctuation = "?!\u{3002}\u{ff0c}\u{2026}";

        assert_eq!(ClassificationResult::UN, classify(punctuation));
        assert_eq!(None, query(punctuation));
        assert!(query_by_english(punctuation).is_empty());
        assert!(query_by_pinyin(punctuation).is_empty());
        assert!(query_by_chinese(punctuation).is_empty());
    }

    #[test]
    fn test_chinese_punctuation_preserves_results_and_order() {
        let expected = query_word_ids("你好");

        assert_eq!(expected, query_word_ids("你好。"));
        assert_eq!(expected, query_word_ids("你好？"));
        assert_eq!(expected, query_word_ids("\u{300c}你好\u{300d}"));
    }

    #[test]
    fn test_classification_uses_normalized_text() {
        for (clean, punctuated) in [
            ("watermelon", "watermelon."),
            ("ni hao", "ni hao."),
            ("ni3 hao3", "ni3 hao3\u{ff1f}"),
            ("你好", "你好\u{3002}"),
            ("xian", "Xi\u{2019}an"),
        ] {
            assert_eq!(classify(clean), classify(punctuated));
        }
    }

    #[test]
    fn test_classify_english() {
        let text = "boat";
        let actual = classify(text);
        let expected = ClassificationResult::EN;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_classify_pinyin_1() {
        let text = "fán tǐ zì";
        let actual = classify(text);
        let expected = ClassificationResult::PY;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_classify_pinyin_2() {
        let text = "fan2ti3zi4";
        let actual = classify(text);
        let expected = ClassificationResult::PY;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_classify_pinyin_3() {
        let text = "jiantizi";
        let actual = classify(text);
        let expected = ClassificationResult::PY;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_classify_simplified() {
        let text = "简体字";
        let actual = classify(text);
        let expected = ClassificationResult::ZH;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_classify_traditional() {
        let text = "繁體字";
        let actual = classify(text);
        let expected = ClassificationResult::ZH;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_simplified_to_traditional() {
        let text = "繁体字";
        let actual = simplified_to_traditional(text);
        let expected = "繁體字";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_traditional_to_simplified() {
        let text = "簡體字";
        let actual = traditional_to_simplified(text);
        let expected = "简体字";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_is_simplified() {
        let text = "简体字";
        let actual = is_simplified(text);
        let expected = true;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_is_not_simplified() {
        let text = "簡體字";
        let actual = is_simplified(text);
        let expected = false;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_is_traditional() {
        let text = "繁體字";
        let actual = is_traditional(text);
        let expected = true;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_is_not_traditional() {
        let text = "繁体字";
        let actual = is_traditional(text);
        let expected = false;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_capitalization() {
        assert_eq!(query_word_ids("watermelon"), query_word_ids("Watermelon"));
        assert_eq!(query_word_ids("beijing"), query_word_ids("Beijing"));
    }

    #[test]
    fn morphology_discovers_the_same_run_concepts() {
        let sets = ["run", "runs", "running", "ran"].map(|query| {
            query_by_english(query)
                .into_iter()
                .map(LexicalUnitRef::id)
                .collect::<HashSet<_>>()
        });
        let shared = sets[0]
            .iter()
            .any(|id| sets[1..].iter().all(|set| set.contains(id)));
        assert!(shared);
    }

    #[test]
    fn optional_grammar_preserves_full_query_coverage() {
        let raw = "to be happy";
        let result = search_english(raw, EnglishSearchOptions::default()).unwrap();
        assert!(result
            .concepts
            .iter()
            .any(|concept| concept.query_bytes == (0..raw.len())));
    }

    #[test]
    fn final_token_completion_respects_submission_boundary() {
        let live = search_english("wat", EnglishSearchOptions::default()).unwrap();
        assert!(live
            .concepts
            .iter()
            .flat_map(|concept| &concept.hits)
            .any(|hit| hit.evidence.completion));

        let submitted = search_english("wat ", EnglishSearchOptions::default()).unwrap();
        assert!(submitted
            .concepts
            .iter()
            .flat_map(|concept| &concept.hits)
            .all(|hit| !hit.evidence.completion));
    }

    #[test]
    fn common_words_remain_searchable() {
        for raw in ["is", "too", "a", "the"] {
            assert!(!query_by_english(raw).is_empty(), "query: {raw}");
        }
    }

    #[test]
    fn spelling_aliases_preserve_digits() {
        let results = query_by_english("3d");
        assert!(results
            .iter()
            .any(|entry| entry.english().any(|definition| definition
                .gloss()
                .value()
                .to_lowercase()
                .contains("3d"))));
    }

    #[test]
    fn persistent_identity_round_trips() {
        let entry = query_by_simplified("西瓜").into_iter().next().unwrap();
        let id = entry.id();
        assert_eq!(Some(entry), query_by_id(&id));
        assert_eq!(Some(entry), query_by_id_str(&id.to_string()));
    }

    #[test]
    fn test_empty_search_chinese() {
        let text = "";
        let result = query_by_chinese(text);
        let length = result.len();
        assert_eq!(length, 0_usize);
    }

    #[test]
    fn test_space_search_chinese() {
        let text = " ";
        let result = query_by_chinese(text);
        let length = result.len();
        assert_eq!(length, 0_usize);
    }

    #[test]
    fn test_empty_search_pinyin() {
        let text = "";
        let result = query_by_pinyin(text);
        let length = result.len();
        assert_eq!(length, 0_usize);
    }

    #[test]
    fn test_space_search_pinyin() {
        let text = " ";
        let result = query_by_pinyin(text);
        let length = result.len();
        assert_eq!(length, 0_usize);
    }

    #[test]
    fn test_empty_search_english() {
        let text = "";
        let result = query_by_english(text);
        let length = result.len();
        assert_eq!(length, 0_usize);
    }

    #[test]
    fn test_space_search_english() {
        let text = " ";
        let result = query_by_english(text);
        let length = result.len();
        assert_eq!(length, 0_usize);
    }

    #[test]
    fn test_no_duplicates() {
        let text = "test";
        let results = query(text).unwrap();
        let mut seen = Vec::new();
        for entry in results {
            assert!(!seen.contains(&entry.id()));
            seen.push(entry.id());
        }
    }

    #[test]
    fn deserializes_hsk_levels_from_generated_dictionary_data() {
        let entry = query_by_simplified("出租车")
            .into_iter()
            .next()
            .expect("generated data should contain 出租车");

        assert_eq!(
            entry.hsk().hsk_2015().collect::<Vec<_>>(),
            vec![HskLevel::One]
        );
        assert_eq!(
            entry.hsk().proficiency_standard_2021().collect::<Vec<_>>(),
            vec![HskLevel::Two]
        );
        assert_eq!(
            entry.hsk().hsk_exam_syllabus_2025().collect::<Vec<_>>(),
            vec![HskLevel::One]
        );
    }

    #[test]
    fn preserves_pinyin_sensitive_hsk_assignments() {
        let entries = query_by_simplified("长");
        let zhang = entries
            .iter()
            .find(|entry| entry.pinyin().numbers() == "zhang3")
            .expect("generated data should contain 长 with zhang3");
        let chang = entries
            .iter()
            .find(|entry| entry.pinyin().numbers() == "chang2")
            .expect("generated data should contain 长 with chang2");

        assert_eq!(
            zhang.hsk().proficiency_standard_2021().collect::<Vec<_>>(),
            vec![HskLevel::Two, HskLevel::Six]
        );
        assert_eq!(
            chang.hsk().proficiency_standard_2021().collect::<Vec<_>>(),
            vec![HskLevel::Two]
        );
    }

    #[test]
    fn serializes_the_shared_advanced_hsk_band() {
        let levels = HskLevels {
            hsk_exam_syllabus_2025: vec![HskLevel::SevenToNine],
            ..HskLevels::default()
        };

        let encoded = serde_json::to_string(&levels).unwrap();
        let decoded: HskLevels = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, levels);
    }
}
