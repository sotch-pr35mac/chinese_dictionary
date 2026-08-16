use bincode::deserialize_from;
pub use character_converter::{
    is_simplified, is_traditional, simplified_to_traditional, traditional_to_simplified,
};
pub use chinese_detection::ClassificationResult;
use fst::raw::Fst;
use fst::Set;
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
static CHINESE_FST: Lazy<Set<&'static [u8]>> =
    Lazy::new(|| Set::new(&include_bytes!("../data/chinese.fst")[..]).unwrap());
static ENGLISH_MAX_LENGTH: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum QueryNormalizationMode {
    General,
    Pinyin,
}

fn is_apostrophe(character: char) -> bool {
    matches!(character, '\'' | '\u{2018}' | '\u{2019}')
}

fn is_sentence_separator(
    character: char,
    previous: Option<char>,
    mode: QueryNormalizationMode,
) -> bool {
    if character == ':'
        && mode == QueryNormalizationMode::Pinyin
        && matches!(previous, Some('u' | 'U'))
    {
        return false;
    }

    matches!(
        character,
        '!' | '"'
            | '('
            | ')'
            | ','
            | '-'
            | '.'
            | '/'
            | ':'
            | ';'
            | '?'
            | '['
            | '\\'
            | ']'
            | '`'
            | '{'
            | '}'
            | '\u{00a1}'
            | '\u{00bf}'
            | '\u{2010}'..='\u{2015}'
            | '\u{2018}'..='\u{201f}'
            | '\u{2026}'
            | '\u{3001}'
            | '\u{3002}'
            | '\u{3008}'..='\u{3011}'
            | '\u{3014}'..='\u{301f}'
            | '\u{fe10}'..='\u{fe16}'
            | '\u{fe35}'..='\u{fe44}'
            | '\u{fe50}'..='\u{fe52}'
            | '\u{fe54}'..='\u{fe57}'
            | '\u{fe59}'..='\u{fe5e}'
            | '\u{ff01}'
            | '\u{ff02}'
            | '\u{ff07}'
            | '\u{ff08}'
            | '\u{ff09}'
            | '\u{ff0c}'
            | '\u{ff0d}'
            | '\u{ff0e}'
            | '\u{ff0f}'
            | '\u{ff1a}'
            | '\u{ff1b}'
            | '\u{ff1f}'
            | '\u{ff3b}'
            | '\u{ff3c}'
            | '\u{ff3d}'
            | '\u{ff40}'
            | '\u{ff5b}'
            | '\u{ff5d}'
    )
}

fn normalize_query(raw: &str, mode: QueryNormalizationMode) -> String {
    let mut normalized = String::with_capacity(raw.len());
    let mut characters = raw.chars().peekable();
    let mut previous: Option<char> = None;
    let mut pending_separator = false;

    while let Some(character) = characters.next() {
        let next = characters.peek().copied();
        let internal_apostrophe = is_apostrophe(character)
            && matches!(previous, Some(character) if character.is_alphanumeric())
            && matches!(next, Some(character) if character.is_alphanumeric());

        if internal_apostrophe {
            previous = Some(character);
            continue;
        }

        if character.is_whitespace()
            || is_apostrophe(character)
            || is_sentence_separator(character, previous, mode)
        {
            pending_separator = !normalized.is_empty();
        } else {
            if pending_separator {
                normalized.push(' ');
                pending_separator = false;
            }
            normalized.push(character);
        }

        previous = Some(character);
    }

    normalized
}

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
    Lazy::force(&CHINESE_FST);
    character_converter::init();
    chinese_detection::init();
}

/// Classify a normalized query as English, Pinyin, Chinese, or uncertain.
/// Sentence punctuation and surrounding whitespace do not affect the result.
pub fn classify(raw: &str) -> ClassificationResult {
    let normalized = normalize_query(raw, QueryNormalizationMode::General);
    if normalized.is_empty() {
        ClassificationResult::UN
    } else {
        chinese_detection::classify(&normalized)
    }
}

/// # Query by English
/// Query the dictionary specifically with English.
/// Normalizes whitespace and sentence punctuation before lookup.
/// Uses a largest first matching approach to look for compound words within the provided string.
/// Will attempt to take the shortest of four tokens or the total number of tokens in the string to match against.
pub fn query_by_english(raw: &str) -> Vec<&'static WordEntry> {
    let normalized = normalize_query(raw, QueryNormalizationMode::General);
    query_by_english_normalized(&normalized)
}

fn query_by_english_normalized(raw: &str) -> Vec<&'static WordEntry> {
    if raw.is_empty() {
        vec![]
    } else {
        let raw = raw.to_lowercase();
        let words: Vec<&str> = raw.split_whitespace().collect();
        let mut entries: Vec<&WordEntry> = Vec::new();
        let default_take = words.len().min(ENGLISH_MAX_LENGTH);
        let mut skip = 0;
        let mut take = default_take;

        while skip < words.len() {
            let substring: String = words
                .iter()
                .copied()
                .skip(skip)
                .take(take)
                .collect::<Vec<_>>()
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
/// Normalizes whitespace, sentence punctuation, and Pinyin apostrophes before lookup.
/// Uses space as a token delineator. Supports pinyin with no tones, tone marks, and tone numbers.
pub fn query_by_pinyin(raw: &str) -> Vec<&'static WordEntry> {
    let normalized = normalize_query(raw, QueryNormalizationMode::Pinyin);
    query_by_pinyin_normalized(&normalized)
}

fn query_by_pinyin_normalized(raw: &str) -> Vec<&'static WordEntry> {
    if raw.is_empty() {
        vec![]
    } else {
        let raw = raw.to_lowercase();
        raw.split_whitespace()
            .flat_map(|word| get_entries(&PINYIN, word))
            .collect::<Vec<_>>()
    }
}

#[inline]
/// Returns the UTF-8 byte length of the longest prefix present in the FST.
fn find_longest_prefix<D: AsRef<[u8]>>(fst: &Fst<D>, value: &[u8]) -> Option<usize> {
    let mut node = fst.root();
    let mut last_match = None;

    for (index, &byte) in value.iter().enumerate() {
        if let Some(transition_index) = node.find_input(byte) {
            node = fst.node(node.transition_addr(transition_index));
            if node.is_final() {
                last_match = Some(index + 1);
            }
        } else {
            return last_match;
        }
    }

    last_match
}

/// # Tokenize Chinese text
/// Segment Chinese text using the headwords in the simplified and traditional dictionaries.
/// Uses greedy longest-prefix matching and omits punctuation or other unindexed text.
pub fn tokenize(raw: &str) -> Vec<&str> {
    let mut tokens = Vec::with_capacity(raw.chars().count());
    let mut skip_bytes = 0;

    while skip_bytes < raw.len() {
        let tail = &raw[skip_bytes..];

        match find_longest_prefix(CHINESE_FST.as_fst(), tail.as_bytes()) {
            Some(length) => {
                let token = &tail[..length];
                tokens.push(token);
                skip_bytes += length;
            }
            None => {
                skip_bytes += tail.chars().next().unwrap().len_utf8();
            }
        }
    }

    tokens.shrink_to_fit();
    tokens
}

/// Queries the exact simplified index first, then appends traditional-only references.
/// References shared by both indexes are deduplicated by their common index ID.
fn get_chinese_entries(word: &str) -> Vec<&'static WordEntry> {
    let simplified_ids = SIMPLIFIED.get(word).map(Vec::as_slice).unwrap_or_default();
    let traditional_ids = TRADITIONAL.get(word).map(Vec::as_slice).unwrap_or_default();

    simplified_ids
        .iter()
        .chain(
            traditional_ids
                .iter()
                .filter(|id| !simplified_ids.contains(id)),
        )
        .map(|id| DATA.get(id).expect("Internal error: Missing definition"))
        .collect()
}

/// # Query by Chinese
/// Query the dictionary specifically with Chinese characters.
/// Supports both Traditional and Simplified Chinese characters.
pub fn query_by_chinese(raw: &str) -> Vec<&'static WordEntry> {
    tokenize(raw)
        .into_iter()
        .flat_map(get_chinese_entries)
        .collect()
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
/// Whitespace and sentence punctuation are normalized before classification and lookup.
///
/// When querying using any of the supported pinyin options, space is used as a token delineator.
///
/// When querying using English, a largest first matching approached is used to look for compound words.
/// Will attempt to take the shortest of four tokens or the total number of tokens in the string to match against.
pub fn query(raw: &str) -> Option<Vec<&'static WordEntry>> {
    let normalized = normalize_query(raw, QueryNormalizationMode::General);
    if normalized.is_empty() {
        return None;
    }

    match chinese_detection::classify(&normalized) {
        ClassificationResult::EN => Some(query_by_english_normalized(&normalized)),
        ClassificationResult::PY => {
            let pinyin = normalize_query(raw, QueryNormalizationMode::Pinyin);
            Some(query_by_pinyin_normalized(&pinyin))
        }
        ClassificationResult::ZH => Some(query_by_chinese(&normalized)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn normalizes_sentence_punctuation_into_token_boundaries() {
        assert_eq!(
            "watermelon",
            normalize_query(
                " \t\u{201c}watermelon\u{201d}\u{ff1f}\n",
                QueryNormalizationMode::General
            )
        );
        assert_eq!(
            "hello world",
            normalize_query("hello,,world", QueryNormalizationMode::General)
        );
        assert_eq!(
            "ni hao",
            normalize_query(
                "ni\u{3001}\u{3001}hao\u{3002}",
                QueryNormalizationMode::General
            )
        );
        assert_eq!(
            "+ < = > ^ | ~",
            normalize_query("+ < = > ^ | ~", QueryNormalizationMode::General)
        );
        assert_eq!(
            "",
            normalize_query(
                "?!\u{3002}\u{ff0c}\u{2026}",
                QueryNormalizationMode::General
            )
        );
    }

    #[test]
    fn canonicalizes_apostrophes_without_splitting_pinyin() {
        assert_eq!(
            "Xian",
            normalize_query("Xi'an", QueryNormalizationMode::General)
        );
        assert_eq!(
            "Xian",
            normalize_query("Xi\u{2019}an", QueryNormalizationMode::Pinyin)
        );
        assert_eq!(
            "Xi1an1",
            normalize_query("Xi1'an1", QueryNormalizationMode::Pinyin)
        );
        assert_eq!(
            "watermelon",
            normalize_query("'watermelon'", QueryNormalizationMode::General)
        );
    }

    #[test]
    fn preserves_u_colon_only_for_pinyin_lookup() {
        assert_eq!(
            "lu 4",
            normalize_query("lu:4", QueryNormalizationMode::General)
        );
        assert_eq!(
            "lu:4",
            normalize_query("lu:4.", QueryNormalizationMode::Pinyin)
        );
    }

    #[test]
    fn chinese_fst_matches_the_union_of_both_indexes() {
        let expected: HashSet<&str> = SIMPLIFIED
            .keys()
            .chain(TRADITIONAL.keys())
            .map(String::as_str)
            .collect();

        assert_eq!(expected.len(), CHINESE_FST.len());
        for headword in expected {
            assert!(
                CHINESE_FST.contains(headword),
                "Missing FST key: {headword}"
            );
        }
    }

    #[test]
    fn every_index_reference_matches_an_embedded_data_entry() {
        for (index_name, dictionary) in [
            ("traditional", &*TRADITIONAL),
            ("simplified", &*SIMPLIFIED),
            ("pinyin", &*PINYIN),
            ("english", &*ENGLISH),
        ] {
            for (key, ids) in dictionary {
                for id in ids {
                    let entry = DATA.get(id).unwrap_or_else(|| {
                        panic!("{index_name} index key {key:?} references missing ID {id}")
                    });
                    assert_eq!(
                        entry.word_id, *id,
                        "{index_name} index key {key:?} references ID {id}, but its entry has word_id {}",
                        entry.word_id
                    );
                }
            }
        }
    }

    #[test]
    fn every_chinese_query_equals_the_deduplicated_union_of_exact_indexes() {
        for headword in SIMPLIFIED.keys().chain(TRADITIONAL.keys()) {
            let mut seen = HashSet::new();
            let expected_ids: Vec<u32> = SIMPLIFIED
                .get(headword)
                .into_iter()
                .flatten()
                .chain(TRADITIONAL.get(headword).into_iter().flatten())
                .copied()
                .filter(|id| seen.insert(*id))
                .collect();
            let actual_ids: Vec<u32> = query_by_chinese(headword)
                .into_iter()
                .map(|entry| entry.word_id)
                .collect();

            assert_eq!(
                expected_ids, actual_ids,
                "Chinese query for {headword:?} did not equal the deduplicated exact-index union"
            );
        }
    }
}
