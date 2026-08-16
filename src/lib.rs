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
//! // Querying the dictionary returns an `Option<Vec<&WordEntry>>`
//! // Read more about the WordEntry struct below
//! let text = "to run";
//! let results = query(text).unwrap();
//! assert_eq!("执行", results[0].simplified);
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
//! #### `WordEntry` struct
//! ```rust
//! extern crate chinese_dictionary;
//!
//! use chinese_dictionary::{MeasureWord, WordEntry};
//!
//! let example_measure_word = MeasureWord {
//!     traditional: "example_traditional".to_string(),
//!     simplified: "example_simplified".to_string(),
//!     pinyin_marks: "example_pinyin_marks".to_string(),
//!     pinyin_numbers: "example_pinyin_numbers".to_string(),
//! };
//!
//! let example = WordEntry {
//!     traditional: "繁體字".to_string(),
//!     simplified: "繁体字".to_string(),
//!     pinyin_marks: "fán tǐ zì".to_string(),
//!     pinyin_numbers: "fan2 ti3 zi4".to_string(),
//!     english: vec!["traditional Chinese character".to_string()],
//!     tone_marks: vec![2 as u8, 3 as u8, 4 as u8],
//!     hash: 000000 as u64,
//!     measure_words: vec![example_measure_word],
//!     hsk: 6 as u8,
//!     word_id: 11111111 as u32,
//! };
//! ```
//!
//! #### `ClassificationResult` enum
//! The possible values for the `ClassificationResult` enum are:
//! - `PY`: Represents Pinyin
//! - `EN`: Represents English
//! - `ZH`: Represents Chinese
//! - `UN`: Represents an uncertain classification result

extern crate bincode;
extern crate character_converter;
extern crate chinese_detection;
extern crate once_cell;

mod chinese_dictionary;
pub use self::chinese_dictionary::{
    classify, init, is_simplified, is_traditional, query, query_by_chinese, query_by_english,
    query_by_pinyin, query_by_simplified, query_by_traditional, simplified_to_traditional,
    tokenize, traditional_to_simplified, ClassificationResult, MeasureWord, WordEntry,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn word_ids(entries: Vec<&WordEntry>) -> Vec<u32> {
        entries.into_iter().map(|entry| entry.word_id).collect()
    }

    fn query_word_ids(raw: &str) -> Option<Vec<u32>> {
        query(raw).map(word_ids)
    }

    fn expected_chinese_ids(headword: &str) -> Vec<u32> {
        let mut seen = HashSet::new();

        query_by_simplified(headword)
            .into_iter()
            .chain(query_by_traditional(headword))
            .filter(|entry| seen.insert(entry.word_id))
            .map(|entry| entry.word_id)
            .collect()
    }

    fn assert_contains_all(actual: Vec<&WordEntry>, expected: Vec<&WordEntry>) {
        let actual_ids: HashSet<u32> = actual.into_iter().map(|entry| entry.word_id).collect();

        for entry in expected {
            assert!(actual_ids.contains(&entry.word_id));
        }
    }

    #[test]
    fn test_search_by_english_1() {
        let text = "watermelon";
        let result = query(text);
        let actual = &result.unwrap().first().unwrap().traditional;
        let expected = "西瓜";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_english_2() {
        let text = "to run";
        let result = query(text);
        let actual = &result.unwrap().first().unwrap().traditional;
        let expected = "執行";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_english_3() {
        let text = "people around the world";
        let result = query(text);
        let actual = &result.unwrap().first().unwrap().traditional;
        let expected = "人們";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_traditional() {
        let text = "繁體字";
        let result = query(text);
        let actual = result.unwrap().first().unwrap().english.first().unwrap();
        let expected = "traditional Chinese character";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_simplified() {
        let text = "龙纹";
        let result = query(text);
        let actual = result.unwrap().first().unwrap().english.first().unwrap();
        let expected = "dragon (as a decorative design)";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_simplified_exact() {
        let text = "龙纹";
        let result = query_by_simplified(text);
        let actual = result.first().unwrap().english.first().unwrap();
        let expected = "dragon (as a decorative design)";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_traditional_exact() {
        let text = "繁體字";
        let result = query_by_traditional(text);
        let actual = result.first().unwrap().english.first().unwrap();
        let expected = "traditional Chinese character";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_sentence() {
        let text = "你好今天的天气还好。";
        let result = query(text);
        let actual = &result.unwrap().first().unwrap().simplified;
        let expected = "你好";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_pinyin_1() {
        let text = "hánlěng";
        let result = query(text);
        let actual = &result.unwrap().first().unwrap().traditional;
        let expected = "寒冷";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_pinyin_2() {
        let text = "dian4nao3";
        let result = query(text);
        let actual = &result.unwrap().first().unwrap().traditional;
        let expected = "電腦";
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_search_by_pinyin_3() {
        let text = "nihao";
        let result = query(text);
        let actual = &result.unwrap().first().unwrap().traditional;
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
        let unique_ids: HashSet<u32> = ids.iter().copied().collect();

        assert_eq!(expected_chinese_ids("万"), ids);
        assert_eq!(3, ids.len());
        assert_eq!(ids.len(), unique_ids.len());
        assert_contains_all(query_by_pinyin("wan4"), query_by_traditional("萬"));
        assert_contains_all(query_by_pinyin("mo4"), query_by_traditional("万"));
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
        let expected = word_ids(query_by_english("people around the world"));

        assert!(!expected.is_empty());
        assert_eq!(
            expected,
            word_ids(query_by_english("  people,around\t the  world. "))
        );
        assert_eq!(
            query_word_ids("people around the world"),
            query_word_ids("people,around the world.")
        );
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
            .any(|entry| entry.simplified == "西安"));
    }

    #[test]
    fn test_pinyin_u_colon_remains_supported() {
        let expected = word_ids(query_by_pinyin("lu:4"));

        assert!(!expected.is_empty());
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
        let english_text = "Watermelon";
        let english_result = query(english_text);
        let english_actual = &english_result.unwrap().first().unwrap().traditional;
        let english_expected = "西瓜";
        assert_eq!(english_expected, english_actual);

        let pinyin_text = "Beijing";
        let pinyin_result = query(pinyin_text);
        let pinyin_actual = &pinyin_result.unwrap().first().unwrap().traditional;
        let pinyin_expected = "北京";
        assert_eq!(pinyin_expected, pinyin_actual);
    }

    #[test]
    fn test_empty_search_chinese() {
        let text = "";
        let result = query_by_chinese(text);
        let length = result.len();
        assert_eq!(length, 0 as usize);
    }

    #[test]
    fn test_space_search_chinese() {
        let text = " ";
        let result = query_by_chinese(text);
        let length = result.len();
        assert_eq!(length, 0 as usize);
    }

    #[test]
    fn test_empty_search_pinyin() {
        let text = "";
        let result = query_by_pinyin(text);
        let length = result.len();
        assert_eq!(length, 0 as usize);
    }

    #[test]
    fn test_space_search_pinyin() {
        let text = " ";
        let result = query_by_pinyin(text);
        let length = result.len();
        assert_eq!(length, 0 as usize);
    }

    #[test]
    fn test_empty_search_english() {
        let text = "";
        let result = query_by_english(text);
        let length = result.len();
        assert_eq!(length, 0 as usize);
    }

    #[test]
    fn test_space_search_english() {
        let text = " ";
        let result = query_by_english(text);
        let length = result.len();
        assert_eq!(length, 0 as usize);
    }

    #[test]
    fn test_no_duplicates() {
        let text = "test";
        let results = query(text).unwrap();
        let mut seen = Vec::new();
        for entry in results {
            assert!(!seen.contains(&entry.word_id));
            seen.push(entry.word_id);
        }
    }
}
