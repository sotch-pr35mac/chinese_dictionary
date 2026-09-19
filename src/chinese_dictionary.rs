use crate::dictionary_archive::{ArchivedDictionaryArchive, ArchivedPostingRange};
use crate::english::{init_english, search_english, EnglishSearchOptions};
use crate::model::{LexicalId, LexicalUnitRef};
pub use character_converter::{
    is_simplified, is_traditional, simplified_to_traditional, traditional_to_simplified,
};
pub use chinese_detection::ClassificationResult;
use fst::raw::Fst;
use fst::Map;
use once_cell::sync::Lazy;
use std::borrow::Cow;

const ARCHIVE_LEN: usize = include_bytes!(concat!(env!("OUT_DIR"), "/dictionary.rkyv")).len();

// The aligned rkyv archive contract requires a 16-byte-aligned base address;
// a plain byte array would only guarantee byte alignment.
#[repr(C, align(16))]
struct AlignedArchive([u8; ARCHIVE_LEN]);

static ARCHIVE_BYTES: AlignedArchive = AlignedArchive(*include_bytes!(concat!(
    env!("OUT_DIR"),
    "/dictionary.rkyv"
)));

/// Accesses the immutable archive that build.rs validated before compilation.
fn archive() -> &'static ArchivedDictionaryArchive {
    // SAFETY: the static wrapper supplies the configured alignment, and
    // build.rs validates this exact immutable artifact against this type.
    unsafe { rkyv::access_unchecked::<ArchivedDictionaryArchive>(&ARCHIVE_BYTES.0) }
}

static CHINESE_FST: Lazy<Map<&'static [u8]>> = Lazy::new(|| {
    Map::new(archive().chinese_fst.as_slice()).expect("build.rs validated the Chinese FST")
});
static PINYIN_FST: Lazy<Map<&'static [u8]>> = Lazy::new(|| {
    Map::new(archive().pinyin_fst.as_slice()).expect("build.rs validated the Pinyin FST")
});

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

fn normalize_query(raw: &str, mode: QueryNormalizationMode) -> Cow<'_, str> {
    let mut previous = None;
    let mut normal_form = true;
    for (index, character) in raw.char_indices() {
        let internal_apostrophe = is_apostrophe(character)
            && previous.is_some_and(char::is_alphanumeric)
            && raw[index + character.len_utf8()..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric);
        if internal_apostrophe
            || is_apostrophe(character)
            || is_sentence_separator(character, previous, mode)
            || (character.is_whitespace()
                && (character != ' '
                    || index == 0
                    || index + 1 == raw.len()
                    || previous == Some(' ')))
        {
            normal_form = false;
            break;
        }
        previous = Some(character);
    }
    if normal_form {
        return Cow::Borrowed(raw);
    }

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

    Cow::Owned(normalized)
}

fn lowercase_query(raw: &str) -> Cow<'_, str> {
    if raw.chars().any(char::is_uppercase) {
        Cow::Owned(raw.to_lowercase())
    } else {
        Cow::Borrowed(raw)
    }
}

/// Initializes auxiliary indexes and touches each embedded archive page.
///
/// This optional warmup avoids constructing an owned lexical corpus. All query
/// functions remain usable without calling `init` first.
pub fn init() {
    Lazy::force(&CHINESE_FST);
    Lazy::force(&PINYIN_FST);
    for offset in (0..ARCHIVE_BYTES.0.len()).step_by(4096) {
        std::hint::black_box(ARCHIVE_BYTES.0[offset]);
    }
    if let Some(last) = ARCHIVE_BYTES.0.last() {
        std::hint::black_box(*last);
    }
    init_english();
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
/// Selects recognized concepts and returns the default bounded, deduplicated result list.
/// Concepts remain in query order. Within each concept, more direct English
/// matches rank first; matches that require completion, alternate spellings,
/// morphology, or other transformations rank lower. Commonness breaks ties
/// between otherwise equally direct matches.
pub fn query_by_english(raw: &str) -> Vec<LexicalUnitRef<'static>> {
    search_english(raw, EnglishSearchOptions::default())
        .map(|result| result.entries)
        .unwrap_or_default()
}

fn posting_slice(
    postings: &'static [rkyv::Archived<u32>],
    range: &ArchivedPostingRange,
) -> &'static [rkyv::Archived<u32>] {
    let start = range.start.to_native() as usize;
    let end = start + range.len.to_native() as usize;
    &postings[start..end]
}

fn sort_by_commonness(entries: &mut [LexicalUnitRef<'static>]) {
    entries.sort_unstable_by(|left, right| {
        right
            .commonness()
            .total_cmp(&left.commonness())
            .then_with(|| left.id().cmp(&right.id()))
    });
}

#[inline]
fn pinyin_entries(word: &str) -> Vec<LexicalUnitRef<'static>> {
    let runtime_keys = PINYIN_FST
        .get(word)
        .and_then(|ordinal| archive().pinyin_postings.get(ordinal as usize))
        .map(|range| posting_slice(archive().pinyin_runtime_keys.as_slice(), range))
        .unwrap_or_default();
    let mut entries = runtime_keys
        .iter()
        .map(|key| unit_by_runtime_key(key.to_native()).expect("validated Pinyin runtime key"))
        .collect::<Vec<_>>();
    sort_by_commonness(&mut entries);
    entries
}

/// # Query by Pinyin
/// Query the dictionary specifically with Pinyin.
/// Normalizes whitespace, sentence punctuation, and Pinyin apostrophes before lookup.
/// Uses space as a token delineator. Supports pinyin with no tones, tone marks, and tone numbers.
/// Token spans remain in query order, with results sorted by descending commonness per span.
pub fn query_by_pinyin(raw: &str) -> Vec<LexicalUnitRef<'static>> {
    let normalized = normalize_query(raw, QueryNormalizationMode::Pinyin);
    query_by_pinyin_normalized(&normalized)
}

fn query_by_pinyin_normalized(raw: &str) -> Vec<LexicalUnitRef<'static>> {
    if raw.is_empty() {
        vec![]
    } else {
        let raw = lowercase_query(raw);
        raw.split_whitespace()
            .flat_map(|word| pinyin_entries(word).into_iter())
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
fn get_chinese_entries(word: &str) -> Vec<LexicalUnitRef<'static>> {
    let Some(postings) = CHINESE_FST
        .get(word)
        .and_then(|ordinal| archive().chinese_postings.get(ordinal as usize))
    else {
        return Vec::new();
    };
    let simplified_ids = posting_slice(
        archive().chinese_runtime_keys.as_slice(),
        &postings.simplified,
    );
    let traditional_ids = posting_slice(
        archive().chinese_runtime_keys.as_slice(),
        &postings.traditional,
    );

    let mut entries = simplified_ids
        .iter()
        .chain(
            traditional_ids
                .iter()
                .filter(|id| !simplified_ids.contains(id)),
        )
        .map(|id| unit_by_runtime_key(id.to_native()).expect("validated Chinese runtime key"))
        .collect::<Vec<_>>();
    sort_by_commonness(&mut entries);
    entries
}

/// # Query by Chinese
/// Query the dictionary specifically with Chinese characters.
/// Supports both Traditional and Simplified Chinese characters.
/// Token spans remain in query order, with results sorted by descending commonness per span.
pub fn query_by_chinese(raw: &str) -> Vec<LexicalUnitRef<'static>> {
    tokenize(raw)
        .into_iter()
        .flat_map(get_chinese_entries)
        .collect()
}

/// # Query by exact Simplified Chinese word
/// Query the Simplified dictionary for a specific word. Does not perform segmentation of input.
/// Results are sorted by descending commonness.
pub fn query_by_simplified(raw: &str) -> Vec<LexicalUnitRef<'static>> {
    let Some(postings) = CHINESE_FST
        .get(raw)
        .and_then(|ordinal| archive().chinese_postings.get(ordinal as usize))
    else {
        return Vec::new();
    };
    let mut entries = posting_slice(
        archive().chinese_runtime_keys.as_slice(),
        &postings.simplified,
    )
    .iter()
    .map(|id| unit_by_runtime_key(id.to_native()).expect("validated Chinese runtime key"))
    .collect::<Vec<_>>();
    sort_by_commonness(&mut entries);
    entries
}

/// # Query by exact Traditional Chinese word
/// Query the Traditional dictionary for a specific word. Does not perform segmentation of input.
/// Results are sorted by descending commonness.
pub fn query_by_traditional(raw: &str) -> Vec<LexicalUnitRef<'static>> {
    let Some(postings) = CHINESE_FST
        .get(raw)
        .and_then(|ordinal| archive().chinese_postings.get(ordinal as usize))
    else {
        return Vec::new();
    };
    let mut entries = posting_slice(
        archive().chinese_runtime_keys.as_slice(),
        &postings.traditional,
    )
    .iter()
    .map(|id| unit_by_runtime_key(id.to_native()).expect("validated Chinese runtime key"))
    .collect::<Vec<_>>();
    sort_by_commonness(&mut entries);
    entries
}

/// # Query
/// Query the dictionary using Traditional Chinese characters, Simplified Chinese characters, English,
/// pinyin with no tone marks, pinyin with tone numbers, and pinyin with tone marks.
///
/// Whitespace and sentence punctuation are normalized before classification and lookup.
///
/// When querying using any of the supported pinyin options, space is used as a token delineator.
///
/// English queries use the lexical concept search with its default limits and completion policy.
pub fn query(raw: &str) -> Option<Vec<LexicalUnitRef<'static>>> {
    let normalized = normalize_query(raw, QueryNormalizationMode::General);
    if normalized.is_empty() {
        return None;
    }

    match chinese_detection::classify(&normalized) {
        ClassificationResult::EN => Some(query_by_english(raw)),
        ClassificationResult::PY => {
            let pinyin = normalize_query(raw, QueryNormalizationMode::Pinyin);
            Some(query_by_pinyin_normalized(&pinyin))
        }
        ClassificationResult::ZH => Some(query_by_chinese(&normalized)),
        _ => None,
    }
}

pub(crate) fn unit_by_runtime_key(runtime_key: u32) -> Option<LexicalUnitRef<'static>> {
    archive()
        .lexical_units
        .get(runtime_key as usize)
        .map(LexicalUnitRef::new)
}

/// Looks up a parsed stable identity with one archived hash-table probe.
pub fn query_by_id(id: &LexicalId) -> Option<LexicalUnitRef<'static>> {
    archive()
        .identities
        .get(id.digest())
        .and_then(|key| unit_by_runtime_key(key.to_native()))
}

/// Parses and looks up the external `1:<lowercase SHA-256 hex>` identity form.
///
/// Parsing uses fixed stack storage and lookup does not allocate.
pub fn query_by_id_str(id: &str) -> Option<LexicalUnitRef<'static>> {
    LexicalId::parse(id).ok().as_ref().and_then(query_by_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fst::Streamer;

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
    fn chinese_fst_matches_its_posting_metadata() {
        assert_eq!(archive().chinese_postings.len(), CHINESE_FST.len());
        let mut stream = CHINESE_FST.stream();
        while let Some((_headword, ordinal)) = stream.next() {
            assert!(archive().chinese_postings.get(ordinal as usize).is_some());
        }
    }

    #[test]
    fn every_index_reference_matches_an_embedded_data_entry() {
        for runtime_key in archive()
            .chinese_runtime_keys
            .iter()
            .chain(archive().pinyin_runtime_keys.iter())
        {
            assert!(
                unit_by_runtime_key(runtime_key.to_native()).is_some(),
                "missing runtime key {}",
                runtime_key.to_native()
            );
        }
        for (digest, runtime_key) in archive().identities.iter() {
            let entry = unit_by_runtime_key(runtime_key.to_native()).unwrap();
            assert_eq!(entry.id().digest(), digest);
        }
    }

    #[test]
    fn borrowed_json_matches_the_owned_model() {
        let entry = query_by_simplified("西瓜").into_iter().next().unwrap();
        assert_eq!(
            serde_json::to_value(entry).unwrap(),
            serde_json::to_value(entry.to_owned()).unwrap()
        );
    }

    #[test]
    fn serializes_ambiguous_classifier_references_with_a_null_identity() {
        let entry = query_by_simplified("细菌")
            .into_iter()
            .find(|entry| entry.traditional() == "細菌")
            .expect("generated data should contain 細菌");
        let serialized = serde_json::to_value(entry).unwrap();
        let definitions = serialized["english"]
            .as_array()
            .expect("serialized entry should contain definitions");
        assert!(definitions
            .iter()
            .all(|definition| definition["measure_words"]
                .as_array()
                .is_none_or(Vec::is_empty)));
        let reference = serialized["measure_words"]
            .as_array()
            .expect("serialized entry should contain entry-level measure words")
            .iter()
            .find(|measure_word| {
                measure_word["value"]["traditional"] == "種"
                    && measure_word["value"]["simplified"] == "种"
            })
            .expect("細菌 should retain Wiktionary's 種／种 classifier");

        assert_eq!(reference["value"]["lexical_id"], serde_json::Value::Null);
        assert!(reference["value"]["varieties"].is_array());
    }

    #[test]
    fn exposes_valid_commonness_scores() {
        for runtime_key in 0..u32::try_from(archive().lexical_units.len()).unwrap() {
            let score = unit_by_runtime_key(runtime_key).unwrap().commonness();
            assert!(score.is_finite() && score >= 0.0);
        }
        assert!(
            query_by_simplified("的")
                .into_iter()
                .any(|entry| entry.commonness() > 0.0),
            "a common corpus word should carry a nonzero score"
        );
    }

    #[test]
    fn chinese_and_pinyin_results_are_frequency_sorted_within_each_span() {
        let chinese_tokens = tokenize("你好我叫");
        assert_eq!(chinese_tokens, ["你好", "我", "叫"]);
        let expected_chinese = chinese_tokens
            .iter()
            .flat_map(|token| query_by_chinese(token))
            .collect::<Vec<_>>();
        assert_eq!(query_by_chinese("你好我叫"), expected_chinese);
        for token in chinese_tokens {
            assert_commonness_descending(&query_by_chinese(token));
        }

        let expected_pinyin = ["ni3", "hao3"]
            .into_iter()
            .flat_map(query_by_pinyin)
            .collect::<Vec<_>>();
        assert_eq!(query_by_pinyin("ni3 hao3"), expected_pinyin);
        assert_commonness_descending(&query_by_pinyin("ni3"));
        assert_commonness_descending(&query_by_pinyin("hao3"));
    }

    fn assert_commonness_descending(entries: &[LexicalUnitRef<'static>]) {
        assert!(!entries.is_empty());
        assert!(entries.windows(2).all(|pair| {
            pair[0].commonness() > pair[1].commonness()
                || (pair[0].commonness() == pair[1].commonness() && pair[0].id() < pair[1].id())
        }));
    }

    #[test]
    fn exposes_paired_simplified_and_traditional_examples() {
        let paired = query_by_simplified("圆满")
            .into_iter()
            .flat_map(LexicalUnitRef::english)
            .flat_map(|definition| definition.examples())
            .map(|example| example.value())
            .find(|example| example.simplified() == Some("圆满结束"))
            .expect("the refreshed corpus should contain a paired 圆满 example");

        assert_eq!(paired.traditional(), Some("圓滿結束"));
        assert_eq!(paired.english(), Some("to come to a successful end"));
    }

    #[test]
    fn every_chinese_query_equals_the_deduplicated_exact_union() {
        let mut stream = CHINESE_FST.stream();
        while let Some((headword, _ordinal)) = stream.next() {
            let headword = std::str::from_utf8(headword).unwrap();
            let mut expected = query_by_simplified(headword);
            for entry in query_by_traditional(headword) {
                if !expected.iter().any(|present| present.id() == entry.id()) {
                    expected.push(entry);
                }
            }
            sort_by_commonness(&mut expected);
            assert_eq!(
                expected,
                query_by_chinese(headword),
                "Chinese query for {headword:?} did not equal the deduplicated exact-index union"
            );
        }
    }
}
