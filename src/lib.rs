// @author	:: Preston Wang-Stosur-Bassett <http://stosur.info>
// @date	:: October 6, 2020
// @description	:: A Chinese / English Dictionary

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
//! use chinese_dictionary::ChineseDictionary;
//!
//! let dictionary = ChineseDictionary::new(); // Instantiation may take a while
//!
//! // Querying the dictionary returns an `Option<Vec<&WordEntry>>`
//! // Read more about the WordEntry struct below
//! let query = "to run";
//! let results = dictionary.query(query).unwrap();
//! let first_result = results.first().unwrap();
//! println!("{}", first_result.simplified) // --> "执行"
//! ```
//!
//! Classifying a string of text
//! ```rust
//! extern crate chinese_dictionary;
//!
//! use chinese_dictionary::ChineseDictionary;
//! use chinese_dictionary::ClassificationResult;
//! 
//! let dictionary = ChineseDictionary::new(); // Instantiation may take a while
//!
//! // Read more about the ClassificationResult enum below 
//! println!("{:?}", dictionary.classify("nihao")); // --> ClassificationResult::PY
//! ```
//! 
//! Convert between Traditional and Simplified Chinese characters
//! ```rust
//! extern crate chinese_dictionary;
//! 
//! use chinese_dictionary::ChineseDictionary;
//! 
//! let dictionary = ChineseDictionary::new(); // Instantiation may take a while
//! 
//! println!("{}", dictionary.convert_to_simplified("簡體字")); // --> "简体字"
//! println!("{}", dictionary.convert_to_traditional("繁体字")); // --> "繁體字"
//! ```
//!
//! Segment a string of characters
//! ```rust
//! extern crate chinese_dictionary;
//! 
//! use chinese_dictionary::ChineseDictionary;
//! 
//! let dictionary = ChineseDictionary::new(); // Instantiation may take a while
//! 
//! println!("{:?}", dictionary.segment("今天天气不错")); // --> ["今天", "天气", "不错"]
//! ```
//!
//! #### `WordEntry` struct
//! ```rust
//! extern crate chinese_dictionary;
//! 
//! use chinese_dictionary::WordEntry;
//! use chinese_dictionary::MeasureWord;
//!
//! let example_measure_word = MeasureWord {
//!	traditional: "example_traditional".to_string(),
//! 	simplified: "example_simplified".to_string(),
//!	pinyin_marks: "example_pinyin_marks".to_string(),
//!	pinyin_numbers: "example_pinyin_numbers".to_string(),
//! };
//! 
//! let example = WordEntry {
//! 	traditional: "繁體字".to_string(),
//!	simplified: "繁体字".to_string(),
//! 	pinyin_marks: "fán tǐ zì".to_string(),
//! 	pinyin_numbers: "fan2 ti3 zi4".to_string(),
//! 	english: vec!["traditional Chinese character".to_string()],
//!	tone_marks: vec![2 as u8, 3 as u8, 4 as u8],
//!	hash: 000000 as u64,
//!	measure_words: vec![example_measure_word],
//!	hsk: 6 as u8,
//!	word_id: 11111111 as u32,
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

mod chinese_dictionary;
pub use self::chinese_dictionary::Dictionary as ChineseDictionary;
pub use self::chinese_dictionary::ClassificationResult;
pub use self::chinese_dictionary::WordEntry;
pub use self::chinese_dictionary::MeasureWord;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn search_by_english_1() {
		let dictionary = ChineseDictionary::new();
		let query = "watermelon";
		let result = dictionary.query(query);
		let actual = &result.unwrap().first().unwrap().traditional;
		let expected = "西瓜";
		assert_eq!(expected, actual);
	}

	#[test]
	fn search_by_english_2() {
		let dictionary = ChineseDictionary::new();
		let query = "to run";
		let result = dictionary.query(query);
		let actual = &result.unwrap().first().unwrap().traditional;
		let expected = "執行";
		assert_eq!(expected, actual);
	}

	#[test]
	fn search_by_english_3() {
		let dictionary = ChineseDictionary::new();
		let query = "people around the world";
		let result = dictionary.query(query);
		let actual = &result.unwrap().first().unwrap().traditional;
		let expected = "世人";
		assert_eq!(expected, actual);
	}

	#[test]
	fn search_by_traditional() {
		let dictionary = ChineseDictionary::new();
		let query = "繁體字";
		let result = dictionary.query(query);
		let actual = result.unwrap().first().unwrap().english.first().unwrap();
		let expected = "traditional Chinese character";
		assert_eq!(expected, actual);
	}

	#[test]
	fn search_by_simplified() {
		let dictionary = ChineseDictionary::new();
		let query = "龙纹";
		let result = dictionary.query(query);
		let actual = result.unwrap().first().unwrap().english.first().unwrap();
		let expected = "dragon (as a decorative design)";
		assert_eq!(expected, actual);
	}
	
	#[test]
	fn search_by_pinyin_1() {
		let dictionary = ChineseDictionary::new();
		let query = "hánlěng";
		let result = dictionary.query(query);
		let actual = &result.unwrap().first().unwrap().traditional;
		let expected = "寒冷";
		assert_eq!(expected, actual);
	}
	
	#[test]
	fn search_by_pinyin_2() {
		let dictionary = ChineseDictionary::new();
		let query = "dian4nao3";
		let result = dictionary.query(query);
		let actual = &result.unwrap().first().unwrap().traditional;
		let expected = "電腦";
		assert_eq!(expected, actual);
	}
	
	#[test]
	fn search_by_pinyin_3() {
		let dictionary = ChineseDictionary::new();
		let query = "nihao";
		let result = dictionary.query(query);
		let actual = &result.unwrap().first().unwrap().traditional;
		let expected = "你好";
		assert_eq!(expected, actual);
	}

	#[test]
	fn segment_traditional() {
		let dictionary = ChineseDictionary::new();
		let sentence = "今天的天氣挺爽";
		let actual = dictionary.segment(sentence);
		let expected = vec!["今天".to_string(), "的".to_string(), "天氣".to_string(), "挺".to_string(), "爽".to_string()];
		assert_eq!(expected, actual);
	}

	#[test]
	fn segment_simplified() {
		let dictionary = ChineseDictionary::new();
		let sentence = "今天的天气挺爽";
		let actual = dictionary.segment(sentence);
		let expected = vec!["今天".to_string(), "的".to_string(), "天气".to_string(), "挺".to_string(), "爽".to_string()];
		assert_eq!(expected, actual);
	}

	#[test]
	fn segment_complex() {
		let dictionary = ChineseDictionary::new();
		let sentence = "红色是我favorite颜色。";
		let actual = dictionary.segment(sentence);
		let expected = vec!["红色".to_string(), "是".to_string(), "我".to_string(), "颜色".to_string()];
		assert_eq!(expected, actual);
	}

	#[test]
	fn classify_english() {
		let dictionary = ChineseDictionary::new();
		let query = "boat";
		let actual = dictionary.classify(query);
		let expected = ClassificationResult::EN;
		assert_eq!(expected, actual);
	}

	#[test]
	fn classify_pinyin_1() {
		let dictionary = ChineseDictionary::new();
		let query = "fán tǐ zì";
		let actual = dictionary.classify(query);
		let expected = ClassificationResult::PY;
		assert_eq!(expected, actual);
	}

	#[test]
	fn classify_pinyin_2() {
		let dictionary = ChineseDictionary::new();
		let query = "fan2ti3zi4";
		let actual = dictionary.classify(query);
		let expected = ClassificationResult::PY;
		assert_eq!(expected, actual);
	}

	#[test]
	fn classify_pinyin_3() {
		let dictionary = ChineseDictionary::new();
		let query = "jiantizi";
		let actual = dictionary.classify(query);
		let expected = ClassificationResult::PY;
		assert_eq!(expected, actual);
	}

	#[test]
	fn classify_simplified() {
		let dictionary = ChineseDictionary::new();
		let query = "简体字";
		let actual = dictionary.classify(query);
		let expected = ClassificationResult::ZH;
		assert_eq!(expected, actual);
	}

	#[test]
	fn classify_traditional() {
		let dictionary = ChineseDictionary::new();
		let query = "繁體字";
		let actual = dictionary.classify(query);
		let expected = ClassificationResult::ZH;
		assert_eq!(expected, actual);
	}

	#[test]
	fn convert_to_traditional() {
		let dictionary = ChineseDictionary::new();
		let query = "繁体字";
		let actual = dictionary.convert_to_traditional(query);
		let expected = "繁體字";
		assert_eq!(expected, actual);
	}

	#[test]
	fn convert_to_simplified() {
		let dictionary = ChineseDictionary::new();
		let query = "簡體字";
		let actual = dictionary.convert_to_simplified(query);
		let expected = "简体字";
		assert_eq!(expected, actual);
	}

	#[test]
	fn is_simplified() {
		let dictionary = ChineseDictionary::new();
		let query = "简体字";
		let actual = dictionary.is_simplified(query);
		let expected = true;
		assert_eq!(expected, actual);
	}

	#[test]
	fn is_not_simplified() {
		let dictionary = ChineseDictionary::new();
		let query = "簡體字";
		let actual = dictionary.is_simplified(query);
		let expected = false;
		assert_eq!(expected, actual);
	}

	#[test]
	fn is_traditional() {
		let dictionary = ChineseDictionary::new();
		let query = "繁體字";
		let actual = dictionary.is_traditional(query);
		let expected = true;
		assert_eq!(expected, actual);
	}

	#[test]
	fn is_not_traditional() {
		let dictionary = ChineseDictionary::new();
		let query = "繁体字";
		let actual = dictionary.is_traditional(query);
		let expected = false;
		assert_eq!(expected, actual);
	}
}
