use crate::chinese_dictionary::unit_by_runtime_key;
use crate::english_search_format::{
    query_views_from_literal, reduce_optional_grammar, tokenize_query, EnglishSearchIndex,
    FormatError, RuntimeKey, TextKey, TokenId, DERIVATION_APOSTROPHE_REMOVED,
    DERIVATION_GRAMMAR_REDUCED, DERIVATION_HYPHENS_JOINED, DERIVATION_HYPHENS_SEPARATED,
    DERIVATION_PARENTHETICAL_OMISSION,
};
use crate::model::LexicalUnitRef;
use fst::{IntoStreamer, Streamer};
use once_cell::sync::Lazy;
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{hash_map::Entry, BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};
use std::ops::Range;

const PREFIX_MINIMUM: usize = 3;
const PREFIX_SCAN_LIMIT: usize = 256;
const EXPANDED_STATE_LIMIT: usize = 4_096;
const OCCURRENCE_LIMIT: usize = 8_192;
const PLAN_LIMIT: usize = 4_096;
const MAXIMUM_RESULT_LIMIT: usize = 200;
const RETAINED_HIT_BATCH_MULTIPLIER: usize = 4;

static INDEX: Lazy<EnglishSearchIndex<'static>> = Lazy::new(|| {
    EnglishSearchIndex::parse(include_bytes!(concat!(env!("OUT_DIR"), "/english.search")))
        .expect("build.rs validated the English search index")
});
static INDEX_LIMITS: Lazy<IndexLimits> = Lazy::new(|| {
    let metadata = INDEX
        .metadata()
        .expect("build.rs validated English search metadata");
    IndexLimits {
        maximum_text_tokens: metadata.maximum_text_tokens as usize,
        maximum_phrase_bytes: metadata.maximum_phrase_bytes as usize,
    }
});
static MORPHOLOGY_TOKENS: Lazy<MorphologyTokenCache> = Lazy::new(|| {
    let mut tokens = Vec::new();
    let mut ranges = Vec::new();
    for family in INDEX.morphology_families() {
        let family = family.expect("build.rs validated morphology families");
        let start = tokens.len();
        tokens.extend(
            family
                .corpus_tokens()
                .map(|token| token.expect("build.rs validated morphology family tokens")),
        );
        ranges.push(start..tokens.len());
    }
    MorphologyTokenCache { tokens, ranges }
});

#[derive(Clone, Copy)]
struct IndexLimits {
    maximum_text_tokens: usize,
    maximum_phrase_bytes: usize,
}

struct MorphologyTokenCache {
    tokens: Vec<TokenId>,
    ranges: Vec<Range<usize>>,
}

impl MorphologyTokenCache {
    fn family(&self, index: usize) -> Option<&[TokenId]> {
        self.ranges
            .get(index)
            .map(|range| &self.tokens[range.clone()])
    }
}

struct IntegerHasher(u64);

impl Default for IntegerHasher {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for IntegerHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        // Runtime keys are u32 and use `write_u32`; retain a deterministic
        // fallback so this hasher remains correct if Hash changes internally.
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
        }
    }

    fn write_u32(&mut self, value: u32) {
        self.0 = u64::from(value);
    }
}

type IntegerMap<V> = HashMap<u32, V, BuildHasherDefault<IntegerHasher>>;

pub(crate) fn init_english() {
    Lazy::force(&INDEX);
    Lazy::force(&INDEX_LIMITS);
    Lazy::force(&MORPHOLOGY_TOKENS);
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Controls whether an unfinished final token may match indexed completions.
pub enum CompletionMode {
    /// Match only complete normalized tokens.
    Disabled,
    #[default]
    /// Complete the final token when it contains at least three characters.
    FinalToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
/// Limits and completion behavior for [`search_english`].
pub struct EnglishSearchOptions {
    /// Completion behavior for the final query token.
    pub completion: CompletionMode,
    /// Maximum number of unique entries returned overall; must be in `1..=200`.
    pub limit: usize,
    /// Maximum hits returned for each concept; must be in `1..=200`.
    pub per_concept_limit: usize,
}

impl Default for EnglishSearchOptions {
    fn default() -> Self {
        Self {
            completion: CompletionMode::FinalToken,
            limit: 50,
            per_concept_limit: 20,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
/// How the selected query span matched a stored English search text.
pub enum EnglishMatchKind {
    /// The query span matched the entire stored search text.
    WholeText,
    /// The query span matched consecutive tokens inside a longer search text.
    ContainedText,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// Ranking evidence retained for one structured English hit.
pub struct EnglishMatchEvidence {
    /// Whether the match covered all or part of a stored search text.
    pub kind: EnglishMatchKind,
    /// Whether final-token completion contributed to this match.
    pub completion: bool,
    /// Normalization derivations applied to the query.
    pub query_derivations: u16,
    /// Normalization derivations recorded on the stored text, when available.
    pub stored_derivations: Option<u16>,
    /// Whether the stored text is a derived spelling or grammar form.
    pub stored_derived: bool,
    /// Whether the stored form omitted parenthetical text.
    pub parenthetical_omission: bool,
    /// Number of transformations applied to the stored form.
    pub stored_transformation_count: u16,
    /// Number of query tokens matched through morphology.
    pub morphology_changes: u16,
    /// Stored tokens surrounding a contained match.
    pub surrounding_tokens: u32,
    /// Characters supplied by final-token completion.
    pub added_characters: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// One ranked hit in a structured English concept.
pub struct EnglishHit {
    /// Index into [`EnglishSearchResult::entries`].
    pub entry_index: usize,
    /// Evidence used to rank this hit.
    pub evidence: EnglishMatchEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// A selected, non-overlapping query span and its ranked returned hits.
pub struct EnglishConcept {
    /// UTF-8 byte range in the original query.
    pub query_bytes: Range<usize>,
    /// Ranked hits whose indices refer to the result's entry array.
    pub hits: Vec<EnglishHit>,
}

#[derive(Debug, Serialize)]
/// Structured English results with borrowed lexical entries.
pub struct EnglishSearchResult {
    /// Selected concepts in query order.
    pub concepts: Vec<EnglishConcept>,
    /// Unique entries merged round-robin across concepts.
    pub entries: Vec<LexicalUnitRef<'static>>,
    /// Whether discovery stopped at a configured work budget.
    ///
    /// Ordinary result limits and proven-safe top-k pruning do not set this.
    pub discovery_truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Failure returned by [`search_english`].
pub enum EnglishSearchError {
    /// The UTF-8 input exceeds the index-supported maximum.
    InputTooLong {
        /// Input length in UTF-8 bytes.
        bytes: usize,
        /// Maximum accepted UTF-8 byte length.
        maximum: usize,
    },
    /// The overall result limit is outside `1..=200`.
    InvalidLimit,
    /// The per-concept result limit is outside `1..=200`.
    InvalidPerConceptLimit,
    /// The embedded search index is malformed or incompatible.
    InvalidIndex(String),
}

impl fmt::Display for EnglishSearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLong { bytes, maximum } => write!(
                formatter,
                "English query is {bytes} bytes; maximum is {maximum}"
            ),
            Self::InvalidLimit => {
                formatter.write_str("English result limit must be between 1 and 200")
            }
            Self::InvalidPerConceptLimit => {
                formatter.write_str("English per-concept limit must be between 1 and 200")
            }
            Self::InvalidIndex(message) => {
                write!(formatter, "invalid English search index: {message}")
            }
        }
    }
}
impl std::error::Error for EnglishSearchError {}
impl From<FormatError> for EnglishSearchError {
    fn from(error: FormatError) -> Self {
        Self::InvalidIndex(error.to_string())
    }
}

#[derive(Clone, Debug)]
struct Group {
    raw_group: u32,
    raw_range: Range<usize>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PlanKind {
    Whole,
    Contained,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Plan {
    kind: PlanKind,
    text_key: Option<TextKey>,
    token_id: Option<TokenId>,
    start_in_text: u32,
    matched_tokens: u32,
    query_flags: u16,
    morphology_changes: u16,
    completion: bool,
    added_characters: u16,
}

#[derive(Clone, Debug)]
struct Candidate {
    start: usize,
    end: usize,
    raw_range: Range<usize>,
    plans: BTreeSet<Plan>,
    best_class: usize,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Rank(u8, u8, u8, u8, u32, u16, u16, u32);

#[derive(Clone, Debug)]
struct RankedHit {
    runtime_key: u32,
    rank: Rank,
    evidence: EnglishMatchEvidence,
}

#[derive(Clone, Copy)]
struct StoredEvidence {
    derivations: Option<u16>,
    derived: bool,
    parenthetical_omission: bool,
    transformation_count: u16,
}

#[derive(Clone, Copy, Debug)]
struct TokenChoice {
    id: TokenId,
    morphed: bool,
    added_characters: u16,
}

struct PreparedForm {
    tokens: Vec<String>,
    alternatives: Vec<Vec<TokenChoice>>,
    literal_text_key: Option<TextKey>,
    flags: u16,
    completion: bool,
}

struct Expansion<'a> {
    plans: &'a mut BTreeSet<Plan>,
    states: &'a mut usize,
    truncated: &'a mut bool,
}

struct Discovery {
    groups: Vec<Group>,
    candidates: Vec<Candidate>,
    truncated: bool,
}

/// Searches English definitions and returns structured concept and evidence data.
///
/// Entry data borrows directly from the embedded dictionary archive. `limit`
/// and `per_concept_limit` must both be in `1..=200`.
pub fn search_english(
    raw: &str,
    options: EnglishSearchOptions,
) -> Result<EnglishSearchResult, EnglishSearchError> {
    if !(1..=MAXIMUM_RESULT_LIMIT).contains(&options.limit) {
        return Err(EnglishSearchError::InvalidLimit);
    }
    if !(1..=MAXIMUM_RESULT_LIMIT).contains(&options.per_concept_limit) {
        return Err(EnglishSearchError::InvalidPerConceptLimit);
    }
    let maximum = 4_096usize.max(INDEX_LIMITS.maximum_phrase_bytes);
    if raw.len() > maximum {
        return Err(EnglishSearchError::InputTooLong {
            bytes: raw.len(),
            maximum,
        });
    }
    let discovery = discover(raw, options.completion)?;
    let selected = choose_concepts(&discovery.groups, &discovery.candidates);
    let mut pools = Vec::new();
    let retained_per_concept = options.limit.min(options.per_concept_limit);
    for candidate_index in selected {
        let candidate = &discovery.candidates[candidate_index];
        let pool = retrieve(candidate, retained_per_concept)?;
        pools.push((candidate, pool));
    }
    let mut entries = Vec::new();
    let mut entry_indices = IntegerMap::default();
    let mut row = 0;
    while entries.len() < options.limit {
        let mut added = false;
        for (_, pool) in &pools {
            if let Some(hit) = pool.get(row) {
                if let Entry::Vacant(entry) = entry_indices.entry(hit.runtime_key) {
                    if let Some(unit) = unit_by_runtime_key(hit.runtime_key) {
                        entry.insert(entries.len());
                        entries.push(unit);
                    }
                }
                added = true;
                if entries.len() == options.limit {
                    break;
                }
            }
        }
        if !added {
            break;
        }
        row += 1;
    }
    let concepts = pools
        .into_iter()
        .map(|(candidate, pool)| EnglishConcept {
            query_bytes: candidate.raw_range.clone(),
            hits: pool
                .into_iter()
                .filter_map(|hit| {
                    entry_indices
                        .get(&hit.runtime_key)
                        .copied()
                        .map(|entry_index| public_hit(hit, entry_index))
                })
                .collect(),
        })
        .collect();
    Ok(EnglishSearchResult {
        concepts,
        entries,
        discovery_truncated: discovery.truncated,
    })
}

fn public_hit(hit: RankedHit, entry_index: usize) -> EnglishHit {
    EnglishHit {
        entry_index,
        evidence: hit.evidence,
    }
}

fn discover(raw: &str, completion_mode: CompletionMode) -> Result<Discovery, EnglishSearchError> {
    let maximum_text_tokens = INDEX_LIMITS.maximum_text_tokens;
    let base = tokenize_query(raw);
    let mut groups = Vec::new();
    for segment in &base {
        for token in &segment.tokens {
            if groups
                .last()
                .is_some_and(|group: &Group| group.raw_group == token.raw_group)
            {
                continue;
            }
            groups.push(Group {
                raw_group: token.raw_group,
                raw_range: token.raw_range.clone(),
            });
        }
    }
    let completion_eligible = completion_mode == CompletionMode::FinalToken;
    let mut by_span = BTreeMap::<(usize, usize), Candidate>::new();
    let group_positions = groups
        .iter()
        .enumerate()
        .map(|(index, group)| (group.raw_group, index))
        .collect::<IntegerMap<_>>();
    let mut expanded_states = 0usize;
    let mut occurrence_records = 0usize;
    let mut planned = 0usize;
    let mut truncated = false;
    let mut alternatives_by_token = BTreeMap::<String, Vec<TokenChoice>>::new();
    'views: for view in query_views_from_literal(&base) {
        for segment in &view.segments {
            if segment.tokens.is_empty() {
                continue;
            }
            let segment_group_ids = segment
                .tokens
                .iter()
                .map(|token| token.raw_group)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let full_span = segment_group_ids
                .first()
                .zip(segment_group_ids.last())
                .and_then(|(first, last)| {
                    Some((
                        *group_positions.get(first)?,
                        *group_positions.get(last)? + 1,
                    ))
                });
            for length in (1..=segment_group_ids.len()).rev() {
                if length > maximum_text_tokens {
                    continue;
                }
                for local_start in 0..=segment_group_ids.len() - length {
                    let first_id = segment_group_ids[local_start];
                    let last_id = segment_group_ids[local_start + length - 1];
                    let Some(&start) = group_positions.get(&first_id) else {
                        continue;
                    };
                    let Some(&last) = group_positions.get(&last_id) else {
                        continue;
                    };
                    let end = last + 1;
                    let literal = segment
                        .tokens
                        .iter()
                        .filter(|token| token.raw_group >= first_id && token.raw_group <= last_id)
                        .map(|token| token.text.clone())
                        .collect::<Vec<_>>();
                    let mut forms = vec![(literal.clone(), view.derivation_flags, false)];
                    let refs = literal.iter().map(String::as_str).collect::<Vec<_>>();
                    if let Some(reduced) = reduce_optional_grammar(&refs) {
                        forms.push((
                            reduced,
                            view.derivation_flags | DERIVATION_GRAMMAR_REDUCED,
                            true,
                        ));
                    }
                    forms.retain(|(tokens, _, _)| {
                        !tokens.is_empty() && tokens.len() <= maximum_text_tokens
                    });
                    if forms.is_empty() {
                        continue;
                    }
                    let is_final = groups[end - 1].raw_range.end == raw.len();
                    let mut prepared_forms = Vec::new();
                    for (tokens, flags, grammar_reduced) in forms {
                        let allow_completion = completion_eligible
                            && is_final
                            && flags == 0
                            && !grammar_reduced
                            && tokens
                                .last()
                                .is_some_and(|token| token.chars().count() >= PREFIX_MINIMUM);
                        let mut alternatives = Vec::with_capacity(tokens.len());
                        for token in &tokens {
                            let choices = if let Some(choices) = alternatives_by_token.get(token) {
                                choices.clone()
                            } else {
                                let choices = token_alternatives(token)?;
                                alternatives_by_token.insert(token.clone(), choices.clone());
                                choices
                            };
                            alternatives.push(choices);
                        }
                        let literal_text_key = INDEX.phrase_key(&tokens.join(" "))?;
                        let exact_viable = alternatives.iter().all(|choices| !choices.is_empty());
                        let completion_viable = allow_completion
                            && alternatives[..alternatives.len() - 1]
                                .iter()
                                .all(|choices| !choices.is_empty());
                        if literal_text_key.is_none() && !exact_viable && !completion_viable {
                            continue;
                        }
                        prepared_forms.push(PreparedForm {
                            tokens,
                            alternatives,
                            literal_text_key,
                            flags,
                            completion: allow_completion,
                        });
                    }
                    if prepared_forms.is_empty() {
                        continue;
                    }
                    if planned >= PLAN_LIMIT {
                        truncated = true;
                        break 'views;
                    }
                    planned += 1;
                    for form in prepared_forms {
                        let mut plans = discover_form(
                            form,
                            &mut expanded_states,
                            &mut occurrence_records,
                            &mut truncated,
                        )?;
                        if plans.is_empty() {
                            continue;
                        }
                        let raw_range =
                            groups[start].raw_range.start..groups[end - 1].raw_range.end;
                        let candidate = by_span.entry((start, end)).or_insert_with(|| Candidate {
                            start,
                            end,
                            raw_range,
                            plans: BTreeSet::new(),
                            best_class: usize::MAX,
                        });
                        candidate.plans.append(&mut plans);
                        if truncated
                            && (expanded_states >= EXPANDED_STATE_LIMIT
                                || occurrence_records >= OCCURRENCE_LIMIT)
                        {
                            break 'views;
                        }
                    }
                }
                // A match spanning this entire hard-boundary segment always
                // beats every subdivision in concept selection: coverage is
                // equal and the squared-span score is strictly larger. Other
                // query views still contribute alternate plans to the full
                // span before making the same safe decision independently.
                if length == segment_group_ids.len()
                    && full_span.is_some_and(|span| by_span.contains_key(&span))
                {
                    break;
                }
            }
        }
    }
    let mut candidates = by_span.into_values().collect::<Vec<_>>();
    for candidate in &mut candidates {
        candidate.best_class = candidate
            .plans
            .iter()
            .map(plan_class)
            .min()
            .unwrap_or(usize::MAX);
    }
    candidates.sort_by_key(|candidate| (candidate.start, candidate.end, candidate.best_class));
    Ok(Discovery {
        groups,
        candidates,
        truncated,
    })
}

fn discover_form(
    form: PreparedForm,
    expanded_states: &mut usize,
    occurrence_records: &mut usize,
    truncated: &mut bool,
) -> Result<BTreeSet<Plan>, EnglishSearchError> {
    let PreparedForm {
        tokens,
        alternatives,
        literal_text_key,
        flags,
        completion,
    } = form;
    let mut plans = BTreeSet::new();
    if let Some(text_key) = literal_text_key {
        plans.insert(Plan {
            kind: PlanKind::Whole,
            text_key: Some(text_key),
            token_id: None,
            start_in_text: 0,
            matched_tokens: tokens.len() as u32,
            query_flags: flags,
            morphology_changes: 0,
            completion: false,
            added_characters: 0,
        });
    }
    if alternatives.iter().all(|choices| !choices.is_empty()) {
        enumerate_whole(
            &alternatives,
            flags,
            false,
            0,
            &mut Vec::new(),
            &mut Expansion {
                plans: &mut plans,
                states: expanded_states,
                truncated,
            },
        )?;
        discover_contained(
            &alternatives,
            flags,
            false,
            &mut plans,
            occurrence_records,
            truncated,
        )?;
        if tokens.len() == 1 {
            for choice in &alternatives[0] {
                plans.insert(Plan {
                    kind: PlanKind::Whole,
                    text_key: None,
                    token_id: Some(choice.id),
                    start_in_text: 0,
                    matched_tokens: 1,
                    query_flags: flags,
                    morphology_changes: u16::from(choice.morphed),
                    completion: false,
                    added_characters: 0,
                });
            }
        }
    }
    if completion {
        let prefix = tokens.last().expect("nonempty");
        if tokens.len() > 1
            && alternatives[..alternatives.len() - 1]
                .iter()
                .all(|choices| !choices.is_empty())
        {
            enumerate_contextual_completions(
                &alternatives[..alternatives.len() - 1],
                prefix,
                flags,
                0,
                &mut Vec::new(),
                &mut Expansion {
                    plans: &mut plans,
                    states: expanded_states,
                    truncated,
                },
            )?;
        }
        let completions = prefix_tokens(prefix, truncated)?;
        if !completions.is_empty() {
            let mut completion_alternatives = alternatives;
            completion_alternatives.pop();
            completion_alternatives.push(completions.clone());
            if completion_alternatives
                .iter()
                .all(|choices| !choices.is_empty())
            {
                enumerate_whole(
                    &completion_alternatives,
                    flags,
                    true,
                    0,
                    &mut Vec::new(),
                    &mut Expansion {
                        plans: &mut plans,
                        states: expanded_states,
                        truncated,
                    },
                )?;
                discover_contained(
                    &completion_alternatives,
                    flags,
                    true,
                    &mut plans,
                    occurrence_records,
                    truncated,
                )?;
                if tokens.len() == 1 {
                    for choice in completions {
                        plans.insert(Plan {
                            kind: PlanKind::Whole,
                            text_key: None,
                            token_id: Some(choice.id),
                            start_in_text: 0,
                            matched_tokens: 1,
                            query_flags: flags,
                            morphology_changes: 0,
                            completion: true,
                            added_characters: choice.added_characters,
                        });
                    }
                }
            }
        }
    }
    Ok(plans)
}

fn enumerate_contextual_completions(
    alternatives: &[Vec<TokenChoice>],
    typed_prefix: &str,
    flags: u16,
    at: usize,
    selected: &mut Vec<TokenChoice>,
    expansion: &mut Expansion<'_>,
) -> Result<(), EnglishSearchError> {
    if at < alternatives.len() {
        for choice in &alternatives[at] {
            if *expansion.states >= EXPANDED_STATE_LIMIT {
                *expansion.truncated = true;
                break;
            }
            selected.push(*choice);
            enumerate_contextual_completions(
                alternatives,
                typed_prefix,
                flags,
                at + 1,
                selected,
                expansion,
            )?;
            selected.pop();
        }
        return Ok(());
    }

    let preceding = phrase_for_choices(selected)?;
    let phrase_prefix = format!("{preceding} {typed_prefix}");
    let map = INDEX.phrase_fst()?;
    let mut stream = map.range().ge(&phrase_prefix).into_stream();
    let typed_characters = typed_prefix.chars().count();
    while let Some((key, value)) = stream.next() {
        if !key.starts_with(phrase_prefix.as_bytes()) {
            break;
        }
        let suffix = &key[phrase_prefix.len()..];
        if suffix.is_empty() || suffix.contains(&b' ') {
            continue;
        }
        *expansion.states += 1;
        if *expansion.states > EXPANDED_STATE_LIMIT {
            *expansion.truncated = true;
            break;
        }
        let completed = std::str::from_utf8(&key[preceding.len() + 1..])
            .map_err(|error| EnglishSearchError::InvalidIndex(error.to_string()))?;
        let added_characters = completed
            .chars()
            .count()
            .saturating_sub(typed_characters)
            .min(u16::MAX as usize) as u16;
        expansion.plans.insert(Plan {
            kind: PlanKind::Whole,
            text_key: Some(TextKey::new(u32::try_from(value).map_err(|_| {
                EnglishSearchError::InvalidIndex("text key overflow".into())
            })?)),
            token_id: None,
            start_in_text: 0,
            matched_tokens: alternatives.len() as u32 + 1,
            query_flags: flags,
            morphology_changes: selected.iter().filter(|choice| choice.morphed).count() as u16,
            completion: true,
            added_characters,
        });
    }
    Ok(())
}

fn token_alternatives(surface: &str) -> Result<Vec<TokenChoice>, EnglishSearchError> {
    let exact = INDEX.token_id(surface)?;
    let mut choices = BTreeMap::new();
    if let Some(id) = exact {
        choices.insert(id, false);
    }
    if let Some(analyses) = INDEX.morphology_analyses(surface)? {
        for family in analyses {
            let family = family?;
            let tokens = MORPHOLOGY_TOKENS
                .family(family.get() as usize)
                .ok_or_else(|| {
                    EnglishSearchError::InvalidIndex("morphology family ID is out of range".into())
                })?;
            for &id in tokens {
                choices.entry(id).or_insert(Some(id) != exact);
            }
        }
    }
    Ok(choices
        .into_iter()
        .map(|(id, morphed)| TokenChoice {
            id,
            morphed,
            added_characters: 0,
        })
        .collect())
}

fn prefix_tokens(
    prefix: &str,
    truncated: &mut bool,
) -> Result<Vec<TokenChoice>, EnglishSearchError> {
    let map = INDEX.token_fst()?;
    let mut stream = map.range().ge(prefix).into_stream();
    let mut result = Vec::new();
    let prefix_characters = prefix.chars().count();
    while let Some((key, value)) = stream.next() {
        if !key.starts_with(prefix.as_bytes()) {
            break;
        }
        if result.len() == PREFIX_SCAN_LIMIT {
            *truncated = true;
            break;
        }
        let completed = std::str::from_utf8(key)
            .map_err(|error| EnglishSearchError::InvalidIndex(error.to_string()))?;
        let added = completed
            .chars()
            .count()
            .saturating_sub(prefix_characters)
            .min(u16::MAX as usize) as u16;
        result.push(TokenChoice {
            id: TokenId::new(
                u32::try_from(value)
                    .map_err(|_| EnglishSearchError::InvalidIndex("token ID overflow".into()))?,
            ),
            morphed: false,
            added_characters: added,
        });
    }
    Ok(result)
}

fn enumerate_whole(
    alternatives: &[Vec<TokenChoice>],
    flags: u16,
    completion: bool,
    at: usize,
    selected: &mut Vec<TokenChoice>,
    expansion: &mut Expansion<'_>,
) -> Result<(), EnglishSearchError> {
    if at == alternatives.len() {
        *expansion.states += 1;
        if *expansion.states > EXPANDED_STATE_LIMIT {
            *expansion.truncated = true;
            return Ok(());
        }
        let phrase = phrase_for_choices(selected)?;
        if let Some(text_key) = INDEX.phrase_key(&phrase)? {
            expansion.plans.insert(Plan {
                kind: PlanKind::Whole,
                text_key: Some(text_key),
                token_id: None,
                start_in_text: 0,
                matched_tokens: alternatives.len() as u32,
                query_flags: flags,
                morphology_changes: selected.iter().filter(|choice| choice.morphed).count() as u16,
                completion,
                added_characters: selected.iter().map(|choice| choice.added_characters).sum(),
            });
        }
        return Ok(());
    }
    for choice in &alternatives[at] {
        if *expansion.states >= EXPANDED_STATE_LIMIT {
            *expansion.truncated = true;
            break;
        }
        selected.push(*choice);
        enumerate_whole(alternatives, flags, completion, at + 1, selected, expansion)?;
        selected.pop();
    }
    Ok(())
}

fn phrase_for_choices(selected: &[TokenChoice]) -> Result<String, EnglishSearchError> {
    let mut phrase = String::new();
    for (index, choice) in selected.iter().enumerate() {
        if index != 0 {
            phrase.push(' ');
        }
        phrase.push_str(INDEX.token_string(choice.id)?);
    }
    Ok(phrase)
}

fn discover_contained(
    alternatives: &[Vec<TokenChoice>],
    flags: u16,
    completion: bool,
    plans: &mut BTreeSet<Plan>,
    occurrence_records: &mut usize,
    truncated: &mut bool,
) -> Result<(), EnglishSearchError> {
    if alternatives.len() < 2 {
        return Ok(());
    }
    let anchor = alternatives
        .iter()
        .enumerate()
        .min_by_key(|(_, choices)| {
            choices
                .iter()
                .filter_map(|choice| {
                    INDEX
                        .token(choice.id)
                        .ok()
                        .map(|token| token.occurrence_count as u64)
                })
                .sum::<u64>()
        })
        .map(|(index, _)| index)
        .unwrap();
    let allowed = alternatives
        .iter()
        .map(|choices| {
            choices
                .iter()
                .map(|choice| choice.id)
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    for anchor_choice in &alternatives[anchor] {
        for occurrence in INDEX.token(anchor_choice.id)?.occurrences()? {
            *occurrence_records += 1;
            if *occurrence_records > OCCURRENCE_LIMIT {
                *truncated = true;
                return Ok(());
            }
            let occurrence = occurrence?;
            let Some(start) = occurrence.position.checked_sub(anchor as u32) else {
                continue;
            };
            let text = INDEX.text(occurrence.text_key)?;
            if start + alternatives.len() as u32 > text.token_count {
                continue;
            }
            let mut morphology_changes = 0_u16;
            let mut added_characters = 0_u16;
            let mut matches = true;
            let mut compared_count = 0_usize;
            let compared_tokens = text.tokens().skip(start as usize).take(alternatives.len());
            for ((matched, allowed), choices) in compared_tokens.zip(&allowed).zip(alternatives) {
                let matched = matched?;
                compared_count += 1;
                if !allowed.contains(&matched) {
                    matches = false;
                    break;
                }
                let choice = choices
                    .iter()
                    .find(|choice| choice.id == matched)
                    .expect("allowed tokens and choices are built together");
                morphology_changes += u16::from(choice.morphed);
                added_characters = added_characters.saturating_add(choice.added_characters);
            }
            if !matches {
                continue;
            }
            if compared_count != alternatives.len() {
                return Err(EnglishSearchError::InvalidIndex(
                    "stored text ended before its declared token count".into(),
                ));
            }
            plans.insert(Plan {
                kind: PlanKind::Contained,
                text_key: Some(occurrence.text_key),
                token_id: None,
                start_in_text: start,
                matched_tokens: alternatives.len() as u32,
                query_flags: flags,
                morphology_changes,
                completion,
                added_characters,
            });
        }
    }
    Ok(())
}

fn plan_class(plan: &Plan) -> usize {
    (usize::from(plan.kind == PlanKind::Contained) * 4)
        + (usize::from(plan.completion) * 2)
        + usize::from(plan.query_flags != 0 || plan.morphology_changes != 0)
}

fn choose_concepts(groups: &[Group], candidates: &[Candidate]) -> Vec<usize> {
    #[derive(Clone)]
    struct State {
        covered: usize,
        squared: usize,
        evidence: [usize; 8],
        selected: Vec<usize>,
    }
    fn compare(left: &State, right: &State, candidates: &[Candidate]) -> Ordering {
        left.covered
            .cmp(&right.covered)
            .then(left.squared.cmp(&right.squared))
            .then(left.evidence.cmp(&right.evidence))
            .then_with(|| {
                let l = left
                    .selected
                    .iter()
                    .map(|index| {
                        (
                            candidates[*index].end - candidates[*index].start,
                            std::cmp::Reverse(*index),
                        )
                    })
                    .collect::<Vec<_>>();
                let r = right
                    .selected
                    .iter()
                    .map(|index| {
                        (
                            candidates[*index].end - candidates[*index].start,
                            std::cmp::Reverse(*index),
                        )
                    })
                    .collect::<Vec<_>>();
                l.cmp(&r)
            })
    }
    let mut states: Vec<Option<State>> = vec![None; groups.len() + 1];
    states[0] = Some(State {
        covered: 0,
        squared: 0,
        evidence: [0; 8],
        selected: Vec::new(),
    });
    for position in 0..groups.len() {
        let Some(state) = states[position].clone() else {
            continue;
        };
        let skip = state.clone();
        if states[position + 1]
            .as_ref()
            .is_none_or(|old| compare(&skip, old, candidates).is_gt())
        {
            states[position + 1] = Some(skip);
        }
        for (index, candidate) in candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.start == position)
        {
            let mut next = state.clone();
            let length = candidate.end - candidate.start;
            next.covered += length;
            next.squared += length * length;
            next.evidence[candidate.best_class.min(7)] += length;
            next.selected.push(index);
            if states[candidate.end]
                .as_ref()
                .is_none_or(|old| compare(&next, old, candidates).is_gt())
            {
                states[candidate.end] = Some(next);
            }
        }
    }
    states
        .pop()
        .flatten()
        .map(|state| state.selected)
        .unwrap_or_default()
}

struct RetainedHits {
    best: IntegerMap<RankedHit>,
    limit: usize,
    batch_size: usize,
    threshold: Option<Rank>,
}

impl RetainedHits {
    fn new(limit: usize) -> Self {
        let batch_size = limit
            .saturating_mul(RETAINED_HIT_BATCH_MULTIPLIER)
            .max(limit + 1);
        let mut best = IntegerMap::default();
        best.reserve(batch_size);
        Self {
            best,
            limit,
            batch_size,
            threshold: None,
        }
    }

    fn add(
        &mut self,
        runtime: RuntimeKey,
        kind: EnglishMatchKind,
        plan: &Plan,
        stored: StoredEvidence,
        surrounding: u32,
    ) {
        let rank = rank_for(runtime, kind, plan, stored, surrounding);
        match self.best.entry(runtime.get()) {
            Entry::Occupied(mut occupied) => {
                if rank < occupied.get().rank {
                    occupied.insert(ranked_hit(runtime, rank, kind, plan, stored, surrounding));
                }
                return;
            }
            Entry::Vacant(vacant) => {
                if self.threshold.is_some_and(|threshold| rank >= threshold) {
                    return;
                }
                vacant.insert(ranked_hit(runtime, rank, kind, plan, stored, surrounding));
            }
        }
        if self.best.len() > self.batch_size {
            self.prune();
        }
    }

    fn prune(&mut self) {
        if self.best.len() <= self.limit {
            return;
        }
        let mut hits = self.best.drain().map(|(_, hit)| hit).collect::<Vec<_>>();
        hits.select_nth_unstable_by(self.limit - 1, |left, right| left.rank.cmp(&right.rank));
        hits.truncate(self.limit);
        self.threshold = hits.iter().map(|hit| hit.rank).max();
        self.best
            .extend(hits.into_iter().map(|hit| (hit.runtime_key, hit)));
    }

    fn finish(mut self) -> Vec<RankedHit> {
        self.prune();
        let mut hits = self.best.into_values().collect::<Vec<_>>();
        hits.sort_unstable_by_key(|hit| hit.rank);
        hits
    }
}

fn retrieve(candidate: &Candidate, limit: usize) -> Result<Vec<RankedHit>, EnglishSearchError> {
    debug_assert!(limit > 0);
    let mut best = RetainedHits::new(limit);
    for plan in &candidate.plans {
        if let Some(token_id) = plan.token_id {
            for hit in INDEX.token(token_id)?.direct_hits() {
                let hit = hit?;
                let kind = if hit.rank.containment {
                    EnglishMatchKind::ContainedText
                } else {
                    EnglishMatchKind::WholeText
                };
                let surrounding = hit.rank.text_token_count.saturating_sub(1);
                best.add(
                    hit.runtime_key,
                    kind,
                    plan,
                    StoredEvidence {
                        derivations: None,
                        derived: hit.rank.derived,
                        parenthetical_omission: hit.rank.parenthetical_omission,
                        transformation_count: hit.rank.transformation_count,
                    },
                    surrounding,
                );
            }
        } else if let Some(text_key) = plan.text_key {
            let text = INDEX.text(text_key)?;
            let surrounding = if plan.kind == PlanKind::Contained {
                text.token_count.saturating_sub(plan.matched_tokens)
            } else {
                0
            };
            let kind = if plan.kind == PlanKind::Contained {
                EnglishMatchKind::ContainedText
            } else {
                EnglishMatchKind::WholeText
            };
            for binding in text.bindings() {
                let binding = binding?;
                best.add(
                    binding.runtime_key,
                    kind,
                    plan,
                    StoredEvidence {
                        derivations: Some(binding.derivation_flags),
                        derived: binding.derivation_flags & alias_mask() != 0,
                        parenthetical_omission: binding.derivation_flags
                            & DERIVATION_PARENTHETICAL_OMISSION
                            != 0,
                        transformation_count: binding.derivation_flags.count_ones() as u16,
                    },
                    surrounding,
                );
            }
        }
    }
    Ok(best.finish())
}

fn rank_for(
    runtime: RuntimeKey,
    kind: EnglishMatchKind,
    plan: &Plan,
    stored: StoredEvidence,
    surrounding: u32,
) -> Rank {
    let aliases = plan.query_flags & alias_mask() != 0 || stored.derived;
    let morphology = plan.morphology_changes != 0;
    let transform_class = match (aliases, morphology) {
        (false, false) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (true, true) => 3,
    };
    let omitted = u8::from(stored.parenthetical_omission);
    let transform_count = stored
        .transformation_count
        .saturating_add(plan.query_flags.count_ones() as u16)
        .saturating_add(plan.morphology_changes);
    Rank(
        u8::from(kind == EnglishMatchKind::ContainedText),
        u8::from(plan.completion),
        transform_class,
        omitted,
        surrounding,
        plan.added_characters,
        transform_count,
        runtime.get(),
    )
}

fn ranked_hit(
    runtime: RuntimeKey,
    rank: Rank,
    kind: EnglishMatchKind,
    plan: &Plan,
    stored: StoredEvidence,
    surrounding: u32,
) -> RankedHit {
    let evidence = EnglishMatchEvidence {
        kind,
        completion: plan.completion,
        query_derivations: plan.query_flags,
        stored_derivations: stored.derivations,
        stored_derived: stored.derived,
        parenthetical_omission: stored.parenthetical_omission,
        stored_transformation_count: stored.transformation_count,
        morphology_changes: plan.morphology_changes,
        surrounding_tokens: surrounding,
        added_characters: plan.added_characters,
    };
    RankedHit {
        runtime_key: runtime.get(),
        rank,
        evidence,
    }
}

fn alias_mask() -> u16 {
    DERIVATION_APOSTROPHE_REMOVED
        | DERIVATION_HYPHENS_JOINED
        | DERIVATION_HYPHENS_SEPARATED
        | DERIVATION_GRAMMAR_REDUCED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impossible_long_spans_do_not_exhaust_discovery_budget() {
        const QUERY_TOKENS: usize = 184;

        let maximum_text_tokens = INDEX.metadata().unwrap().maximum_text_tokens as usize;
        let phrase_fst = INDEX.phrase_fst().unwrap();
        let mut phrases = phrase_fst.into_stream();
        let mut maximum_phrase = None;
        while let Some((bytes, _)) = phrases.next() {
            let phrase = std::str::from_utf8(bytes).unwrap();
            if phrase.split_whitespace().count() == maximum_text_tokens {
                maximum_phrase = Some(phrase.to_owned());
                break;
            }
        }
        let maximum_phrase =
            maximum_phrase.expect("the index metadata maximum must be attained by a phrase");
        assert!(maximum_text_tokens < QUERY_TOKENS);

        let filler_count = QUERY_TOKENS - maximum_text_tokens;
        let mut raw = std::iter::repeat_n("qzxv", filler_count)
            .collect::<Vec<_>>()
            .join(" ");
        raw.push(' ');
        let phrase_start = raw.len();
        raw.push_str(&maximum_phrase);
        assert_eq!(raw.split_whitespace().count(), QUERY_TOKENS);

        let result = search_english(&raw, EnglishSearchOptions::default()).unwrap();
        assert!(result.concepts.iter().any(|concept| {
            concept.query_bytes == (phrase_start..raw.len()) && !concept.hits.is_empty()
        }));
    }

    #[test]
    fn unavailable_tokens_do_not_exhaust_discovery_budget() {
        let mut raw = std::iter::repeat_n("qzxv", 90)
            .collect::<Vec<_>>()
            .join(" ");
        raw.push(' ');
        let watermelon_start = raw.len();
        raw.push_str("watermelon");

        let result = search_english(&raw, EnglishSearchOptions::default()).unwrap();
        assert!(!result.discovery_truncated);
        assert!(result.concepts.iter().any(|concept| {
            concept.query_bytes == (watermelon_start..raw.len()) && !concept.hits.is_empty()
        }));
    }

    #[test]
    fn limits_outside_supported_range_are_rejected() {
        for limit in [0, 201] {
            let options = EnglishSearchOptions {
                limit,
                ..EnglishSearchOptions::default()
            };
            assert!(matches!(
                search_english("run", options),
                Err(EnglishSearchError::InvalidLimit)
            ));
        }
        for per_concept_limit in [0, 201] {
            let options = EnglishSearchOptions {
                per_concept_limit,
                ..EnglishSearchOptions::default()
            };
            assert!(matches!(
                search_english("run", options),
                Err(EnglishSearchError::InvalidPerConceptLimit)
            ));
        }
        assert_eq!(
            EnglishSearchError::InvalidPerConceptLimit.to_string(),
            "English per-concept limit must be between 1 and 200"
        );
    }

    #[test]
    fn defaults_and_boundary_limits_are_supported() {
        assert_eq!(EnglishSearchOptions::default().limit, 50);
        assert_eq!(EnglishSearchOptions::default().per_concept_limit, 20);
        for limit in [1, 20, 50, 200] {
            let result = search_english(
                "run",
                EnglishSearchOptions {
                    limit,
                    per_concept_limit: limit,
                    ..EnglishSearchOptions::default()
                },
            )
            .unwrap();
            assert!(result.entries.len() <= limit);
            assert!(result
                .concepts
                .iter()
                .all(|concept| concept.hits.len() <= limit));
        }
    }

    #[test]
    fn structured_hits_reference_returned_entries() {
        let result = search_english(
            "watermelon computer",
            EnglishSearchOptions {
                completion: CompletionMode::Disabled,
                limit: 3,
                per_concept_limit: 20,
            },
        )
        .unwrap();
        assert_eq!(result.entries.len(), 3);
        assert!(result
            .concepts
            .iter()
            .flat_map(|concept| &concept.hits)
            .all(|hit| hit.entry_index < result.entries.len()));
    }

    #[test]
    fn bounded_retention_matches_exhaustive_sorting() {
        let discovery = discover("run", CompletionMode::Disabled).unwrap();
        let selected = choose_concepts(&discovery.groups, &discovery.candidates);
        let candidate = &discovery.candidates[selected[0]];
        let expected = retrieve(candidate, MAXIMUM_RESULT_LIMIT).unwrap();
        for limit in [1, 20, 50] {
            let actual = retrieve(candidate, limit).unwrap();
            assert_eq!(
                actual.iter().map(|hit| hit.rank).collect::<Vec<_>>(),
                expected
                    .iter()
                    .take(limit)
                    .map(|hit| hit.rank)
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn bounded_retention_matches_adversarial_exhaustive_reference() {
        let plan = Plan {
            kind: PlanKind::Whole,
            text_key: None,
            token_id: Some(TokenId::new(0)),
            start_in_text: 0,
            matched_tokens: 1,
            query_flags: 0,
            morphology_changes: 0,
            completion: false,
            added_characters: 0,
        };
        let stored = |derivations| StoredEvidence {
            derivations,
            derived: false,
            parenthetical_omission: false,
            transformation_count: 0,
        };
        let mut inputs = (10_u32..=25)
            .map(|key| (key, key, stored(Some(1))))
            .collect::<Vec<_>>();
        // These arrive after the first pruning batch. One improves an evicted
        // key enough to re-enter; one improves a retained key.
        inputs.push((25, 0, stored(Some(2))));
        inputs.push((10, 1, stored(Some(2))));
        // Equal rank must preserve the first evidence deterministically.
        inputs.push((25, 0, stored(Some(4))));

        let mut retained = RetainedHits::new(3);
        let mut exhaustive = IntegerMap::<RankedHit>::default();
        for &(key, surrounding, evidence) in &inputs {
            let runtime = RuntimeKey::new(key);
            retained.add(
                runtime,
                EnglishMatchKind::WholeText,
                &plan,
                evidence,
                surrounding,
            );
            let rank = rank_for(
                runtime,
                EnglishMatchKind::WholeText,
                &plan,
                evidence,
                surrounding,
            );
            let candidate = ranked_hit(
                runtime,
                rank,
                EnglishMatchKind::WholeText,
                &plan,
                evidence,
                surrounding,
            );
            exhaustive
                .entry(key)
                .and_modify(|old| {
                    if rank < old.rank {
                        *old = candidate.clone();
                    }
                })
                .or_insert(candidate);
        }
        let mut expected = exhaustive.into_values().collect::<Vec<_>>();
        expected.sort_unstable_by_key(|hit| hit.rank);
        expected.truncate(3);
        let actual = retained.finish();

        assert_eq!(
            actual.iter().map(|hit| hit.rank).collect::<Vec<_>>(),
            expected.iter().map(|hit| hit.rank).collect::<Vec<_>>()
        );
        assert_eq!(actual[0].runtime_key, 25);
        assert_eq!(actual[0].evidence.stored_derivations, Some(2));
    }
}
