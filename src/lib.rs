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
        let actual = result.unwrap().first().unwrap().english.first().unwrap();
        let expected = "hello";
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
