// @author		:: Preston Wang-Stosur-Bassett <p.wanstobas@gmail.com>
// @date		:: October 17, 2020
// @description		:: A Chinese / English Dictionary

use bincode::deserialize_from;
use character_converter::CharacterConverter;
use chinese_detection::ChineseDetection;
use serde_derive::Deserialize;
use std::collections::HashMap;
pub use chinese_detection::ClassificationResult;

static TRADITIONAL: &'static [u8] = include_bytes!("../data/traditional.dictionary");
static SIMPLIFIED: &'static [u8] = include_bytes!("../data/simplified.dictionary");
static PINYIN: &'static [u8] = include_bytes!("../data/pinyin.dictionary");
static ENGLISH: &'static [u8] = include_bytes!("../data/english.dictionary");
static DATA: &'static [u8] = include_bytes!("../data/data.dictionary");
static ENGLISH_MAX_LENGTH: usize = 4;

#[derive(Deserialize, Debug)]
pub struct MeasureWord {
	pub traditional: String,
	pub simplified: String, 
	pub pinyin_marks: String,
	pub pinyin_numbers: String
}

#[derive(Deserialize, Debug)]
pub struct WordEntry {
	pub traditional: String,
	pub simplified: String,
	pub pinyin_marks: String,
	pub pinyin_numbers: String,
	pub english: Vec<String>,
	pub tone_marks: Vec<u8>, 
	pub hash: u64,
	pub measure_words: Vec<MeasureWord>,
	pub hsk: u8,
	pub word_id: u32
}

pub struct Dictionary {
	traditional: HashMap<String, Vec<u32>>,
	simplified: HashMap<String, Vec<u32>>,
	pinyin: HashMap<String, Vec<u32>>,
	english: HashMap<String, Vec<u32>>,
	data: HashMap<u32, WordEntry>,
	character_util: CharacterConverter,
	language_util: ChineseDetection,
}

impl Dictionary {
	pub fn new() -> Dictionary {
		Dictionary {
			traditional: deserialize_from(TRADITIONAL).unwrap(),
			simplified: deserialize_from(SIMPLIFIED).unwrap(),
			pinyin: deserialize_from(PINYIN).unwrap(),
			english: deserialize_from(ENGLISH).unwrap(),
			data: deserialize_from(DATA).unwrap(),
			character_util: CharacterConverter::new(), // This operation takes 1.5 seconds to complete
			language_util: ChineseDetection::new(), // This operation takes 2 seconds to complete
		}
	}

	/// # Classify
	/// Classify a string of text as either Pinyin, English, or Chinese characters.
	/// For more information on the possible `ClassificationResult` enum values refer to the README.
	pub fn classify(&self, raw: &str) -> ClassificationResult {
		self.language_util.classify(raw)
	}

	/// # Convert to Simplified
	/// Convert a string of Traditional Chinese characters to their Simplified form.
	pub fn convert_to_simplified(&self, raw: &str) -> String {
		self.character_util.traditional_to_simplified(raw)
	}

	/// # Convert to Traditional
	/// Convert a string of Simplified Chinese characters to their Traditional form.
	pub fn convert_to_traditional(&self, raw: &str) -> String {
		self.character_util.simplified_to_traditional(raw)
	}

	/// # Is Traditional
	/// Checks if a string of Chinese characters is Traditional
	pub fn is_traditional(&self, raw: &str) -> bool {
		self.character_util.is_traditional(raw)
	}

	/// # Is Simplified
	/// Checks if a string of Chinese characters is Simplified
	pub fn is_simplified(&self, raw: &str) -> bool {
		self.character_util.is_simplified(raw)
	}

	/// # Segment
	/// Segment a string of either Traditional or Simplified Chinese characters into constituent words.
	/// Uses a largest first matching dictionary driven approach.
	pub fn segment(&self, raw: &str) -> Vec<String> {
		let mut tokens: Vec<String> = Vec::new();
		let default_take = if raw.len() < 20 { raw.len() } else { 20 };
		let mut skip = 0;
		let mut take = default_take;
		let dictionary = if self.character_util.is_simplified(raw) { &self.simplified } else { &self.traditional };
		
		while skip < raw.chars().count() {
			let substring: String = raw.chars().skip(skip).take(take).collect();
			if !dictionary.contains_key(&substring) {
				if take > 1 {
					take -= 1;
				} else {
					skip +=1;
					take = default_take;
				}
			} else {
				tokens.push(substring);
				skip += take;
				take = default_take;
			}
		}
		
		return tokens;
	}

	/// # Query by English
	/// Query the dictionary specifically with English.
	/// Uses a largest first matching approach to look for compound words within the provided string.
	/// Will attempt to take the shortest of four tokens or the total number of tokens in the string to match against.
	pub fn query_by_english(&self, raw: &str) -> Vec<&WordEntry> {
		let mut entries: Vec<&WordEntry> = Vec::new();
		let default_take = if raw.split(" ").count() < ENGLISH_MAX_LENGTH { raw.split(" ").count() } else { ENGLISH_MAX_LENGTH };
		let mut skip = 0;
		let mut take = default_take;

		while skip < raw.split(" ").count() {
			let substring: String = raw.split(" ").skip(skip).take(take).collect::<Vec<&str>>().join("%20");
			if !self.english.contains_key(&substring) {
				if take > 1 {
					take -= 1;
				} else {
					skip += 1;
					take = default_take;	
				}
			} else {
				for item in self.english.get(&substring).unwrap() {
					entries.push(self.data.get(item).unwrap());
				}
				skip += take;
				take = default_take;
			}
		}

		return entries;
	}

	/// # Query by Pinyin
	/// Query the dictionary specifically with Pinyin.
	/// Uses space as a token delineator. Supports pinyin with no tones, tone marks, and tone numbers.
	pub fn query_by_pinyin(&self, raw: &str) -> Vec<&WordEntry> {
		let mut entries: Vec<&WordEntry> = Vec::new();
		
		for word in raw.split(" ") {
			if self.pinyin.contains_key(word) {
				for item in self.pinyin.get(word).unwrap() {
					entries.push(self.data.get(item).unwrap());
				}
			}	
		}
		
		return entries;
	}

	fn query_by_characters(&self, dictionary: &HashMap<String, Vec<u32>>, raw: &str) -> Vec<&WordEntry> {
		let mut entries: Vec<&WordEntry> = Vec::new();

		for word in self.segment(raw) {
			if dictionary.contains_key(&word) {
				for item in dictionary.get(raw).unwrap() {
					entries.push(self.data.get(item).unwrap());
				}
			}
		}

		return entries;
	}
	
	fn query_by_traditional(&self, raw: &str) -> Vec<&WordEntry> {
		self.query_by_characters(&self.traditional, raw)
	}
	
	fn query_by_simplified(&self, raw: &str) -> Vec<&WordEntry> {
		self.query_by_characters(&self.simplified, raw)
	}

	/// # Query by Chinese
	/// Query the dictionary specifically with Chinese characters.
	/// Supports both Traditional and Simplified Chinese characters.
	pub fn query_by_chinese(&self, raw: &str) -> Vec<&WordEntry> {
		match self.character_util.is_traditional(raw) {
			true => self.query_by_traditional(raw),
			false => self.query_by_simplified(raw)	
		}	
	}

	/// # Query
	/// Query the dictionary using Traditional Chinese characters, Simplified Chinese characters, English,
	/// pinyin with no tone marks, pinyin with tone numbers, and pinyin with tone marks. 
	///
	/// When querying using any of the supported pinyin options, space is used as a token delineator.
	///
	/// When querying using English, a largest first matching approached is used to look for compound words.
	/// Will attempt to take the shortest of four tokens or the total number of tokens in the string to match against. 
	pub fn query(&self, raw: &str) -> Option<Vec<&WordEntry>> {
		match self.language_util.classify(raw) {
			ClassificationResult::EN => Some(self.query_by_english(raw)),
			ClassificationResult::PY => Some(self.query_by_pinyin(raw)),
			ClassificationResult::ZH => Some(self.query_by_chinese(raw)),
			_ => None
		}
	}
}
