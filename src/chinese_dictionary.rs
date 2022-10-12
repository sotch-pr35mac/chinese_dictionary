use bincode::deserialize_from;
pub use character_converter::{
    is_simplified, is_traditional, simplified_to_traditional, tokenize, traditional_to_simplified,
};
pub use chinese_detection::{classify, ClassificationResult};
use once_cell::sync::Lazy;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;

type Searchable = HashMap<String, Vec<u32>>;

static TRADITIONAL: Lazy<Searchable> =
    Lazy::new(|| deserialize_from(&include_bytes!("../data/traditional.dictionary")[..]).unwrap());
static SIMPLIFIED: Lazy<Searchable> =
    Lazy::new(|| deserialize_from(&include_bytes!("../data/simplified.dictionary")[..]).unwrap());
static PINYIN: Lazy<Searchable> =
    Lazy::new(|| deserialize_from(&include_bytes!("../data/pinyin.dictionary")[..]).unwrap());
static ENGLISH: Lazy<Searchable> =
    Lazy::new(|| deserialize_from(&include_bytes!("../data/english.dictionary")[..]).unwrap());
static DATA: Lazy<HashMap<u32, WordEntry>> =
    Lazy::new(|| deserialize_from(&include_bytes!("../data/data.dictionary")[..]).unwrap());
static ENGLISH_MAX_LENGTH: usize = 4;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MeasureWord {
    pub traditional: String,
    pub simplified: String,
    pub pinyin_marks: String,
    pub pinyin_numbers: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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
    pub word_id: u32,
}

pub fn init() {
    Lazy::force(&TRADITIONAL);
    Lazy::force(&SIMPLIFIED);
    Lazy::force(&PINYIN);
    Lazy::force(&ENGLISH);
    Lazy::force(&DATA);
    character_converter::init();
    chinese_detection::init();
}

/// # Query by English
/// Query the dictionary specifically with English.
/// Uses a largest first matching approach to look for compound words within the provided string.
/// Will attempt to take the shortest of four tokens or the total number of tokens in the string to match against.
pub fn query_by_english(raw: &str) -> Vec<&'static WordEntry> {
    if raw.is_empty() || raw == " " {
        vec![]
    } else {
        let raw = raw.to_lowercase();
        let mut entries: Vec<&WordEntry> = Vec::new();
        let default_take = if raw.split(' ').count() < ENGLISH_MAX_LENGTH {
            raw.split(' ').count()
        } else {
            ENGLISH_MAX_LENGTH
        };
        let mut skip = 0;
        let mut take = default_take;

        while skip < raw.split(' ').count() {
            let substring: String = raw
                .split(' ')
                .skip(skip)
                .take(take)
                .collect::<Vec<&str>>()
                .join("%20");
            if !ENGLISH.contains_key(&substring) {
                if take > 1 {
                    take -= 1;
                } else {
                    skip += 1;
                    take = default_take;
                }
            } else {
                for item in ENGLISH.get(&substring).unwrap() {
                    entries.push(DATA.get(item).unwrap());
                }
                skip += take;
                take = default_take;
            }
        }

        entries.dedup();
        entries
    }
}

#[inline]
fn get_entries<'a>(dict: &'a Searchable, word: &str) -> impl Iterator<Item = &'a WordEntry> {
    static EMPTY: Vec<u32> = Vec::new();
    dict.get(word)
        .unwrap_or(&EMPTY)
        .iter()
        .map(|k| DATA.get(k).expect("Internal error: Missing definition"))
}

/// # Query by Pinyin
/// Query the dictionary specifically with Pinyin.
/// Uses space as a token delineator. Supports pinyin with no tones, tone marks, and tone numbers.
pub fn query_by_pinyin(raw: &str) -> Vec<&'static WordEntry> {
    if raw.is_empty() || raw == " " {
        vec![]
    } else {
        let raw = raw.to_lowercase();
        raw.split(' ')
            .flat_map(|word| get_entries(&PINYIN, word))
            .collect::<Vec<_>>()
    }
}

fn query_by_characters(dictionary: &'static Searchable, raw: &str) -> Vec<&'static WordEntry> {
    tokenize(raw)
        .into_iter()
        .flat_map(|word| get_entries(dictionary, word))
        .collect::<Vec<_>>()
}

/// # Query by Chinese
/// Query the dictionary specifically with Chinese characters.
/// Supports both Traditional and Simplified Chinese characters.
pub fn query_by_chinese(raw: &str) -> Vec<&'static WordEntry> {
    query_by_characters(
        if is_traditional(raw) {
            &TRADITIONAL
        } else {
            &SIMPLIFIED
        },
        raw,
    )
}

/// # Query by exact Simplified Chinese word
/// Query the Simplified dictionary for a specific word. Does not perform segmentation of input.
pub fn query_by_simplified(raw: &str) -> Vec<&'static WordEntry> {
    get_entries(&SIMPLIFIED, raw).collect::<Vec<_>>()
}

/// # Query by exact Traditional Chinese word
/// Query the Traditional dictionary for a specific word. Does not perform segmentation of input.
pub fn query_by_traditional(raw: &str) -> Vec<&'static WordEntry> {
    get_entries(&TRADITIONAL, raw).collect::<Vec<_>>()
}

/// # Query
/// Query the dictionary using Traditional Chinese characters, Simplified Chinese characters, English,
/// pinyin with no tone marks, pinyin with tone numbers, and pinyin with tone marks.
///
/// When querying using any of the supported pinyin options, space is used as a token delineator.
///
/// When querying using English, a largest first matching approached is used to look for compound words.
/// Will attempt to take the shortest of four tokens or the total number of tokens in the string to match against.
pub fn query(raw: &str) -> Option<Vec<&'static WordEntry>> {
    match chinese_detection::classify(raw) {
        ClassificationResult::EN => Some(query_by_english(raw)),
        ClassificationResult::PY => Some(query_by_pinyin(raw)),
        ClassificationResult::ZH => Some(query_by_chinese(raw)),
        _ => None,
    }
}
