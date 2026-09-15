use crate::chinese_dictionary::unit_by_runtime_key;
use crate::english_search_format::{
    query_views, reduce_optional_grammar, tokenize_query, EnglishSearchIndex, FormatError,
    RuntimeKey, TextKey, TokenId, DERIVATION_APOSTROPHE_REMOVED, DERIVATION_GRAMMAR_REDUCED,
    DERIVATION_HYPHENS_JOINED, DERIVATION_HYPHENS_SEPARATED, DERIVATION_PARENTHETICAL_OMISSION,
};
use crate::model::LexicalUnit;
use fst::{IntoStreamer, Streamer};
use once_cell::sync::Lazy;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

const PREFIX_MINIMUM: usize = 3;
const PREFIX_SCAN_LIMIT: usize = 256;
const EXPANDED_STATE_LIMIT: usize = 4_096;
const OCCURRENCE_LIMIT: usize = 8_192;
const PLAN_LIMIT: usize = 4_096;
const CANDIDATE_LIMIT: usize = 2_048;

static INDEX: Lazy<EnglishSearchIndex<'static>> = Lazy::new(|| {
    EnglishSearchIndex::parse(include_bytes!(concat!(env!("OUT_DIR"), "/english.search")))
        .expect("build.rs validated the English search index")
});
static MORPHOLOGY_TOKENS: Lazy<Vec<Vec<TokenId>>> = Lazy::new(|| {
    INDEX
        .morphology_families()
        .map(|family| {
            family
                .expect("build.rs validated morphology families")
                .corpus_tokens()
                .collect::<Result<Vec<_>, _>>()
                .expect("build.rs validated morphology family tokens")
        })
        .collect()
});

pub(crate) fn init_english() {
    Lazy::force(&INDEX);
    Lazy::force(&MORPHOLOGY_TOKENS);
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CompletionMode {
    Disabled,
    #[default]
    FinalToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnglishSearchOptions {
    pub completion: CompletionMode,
    pub limit: usize,
    /// Maximum hits returned for each concept; must be positive.
    pub per_concept_limit: usize,
}

impl Default for EnglishSearchOptions {
    fn default() -> Self {
        Self {
            completion: CompletionMode::FinalToken,
            limit: 100,
            per_concept_limit: 20,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EnglishMatchKind {
    WholeText,
    ContainedText,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnglishMatchEvidence {
    pub kind: EnglishMatchKind,
    pub completion: bool,
    pub query_derivations: u16,
    pub stored_derivations: Option<u16>,
    pub stored_derived: bool,
    pub parenthetical_omission: bool,
    pub stored_transformation_count: u16,
    pub morphology_changes: u16,
    pub surrounding_tokens: u32,
    pub added_characters: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnglishHit {
    pub runtime_key: u32,
    pub evidence: EnglishMatchEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnglishCursor {
    raw: String,
    options: EnglishSearchOptions,
    query_bytes: Range<usize>,
    offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnglishConcept {
    pub query_bytes: Range<usize>,
    pub hits: Vec<EnglishHit>,
    pub next_cursor: Option<EnglishCursor>,
}

#[derive(Debug)]
pub struct EnglishSearchResult {
    pub concepts: Vec<EnglishConcept>,
    pub entries: Vec<&'static LexicalUnit>,
    pub discovery_truncated: bool,
    pub has_more: bool,
}

#[derive(Debug)]
pub struct EnglishPage {
    pub query_bytes: Range<usize>,
    pub hits: Vec<EnglishHit>,
    pub entries: Vec<&'static LexicalUnit>,
    pub next_cursor: Option<EnglishCursor>,
    pub discovery_truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnglishSearchError {
    InputTooLong {
        bytes: usize,
        maximum: usize,
    },
    /// A per-concept page size of zero cannot make pagination progress.
    InvalidPerConceptLimit,
    InvalidIndex(String),
    StaleCursor,
}

impl fmt::Display for EnglishSearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLong { bytes, maximum } => write!(
                formatter,
                "English query is {bytes} bytes; maximum is {maximum}"
            ),
            Self::InvalidPerConceptLimit => {
                formatter.write_str("English per-concept limit must be positive")
            }
            Self::InvalidIndex(message) => {
                write!(formatter, "invalid English search index: {message}")
            }
            Self::StaleCursor => {
                formatter.write_str("English search cursor no longer matches its query plan")
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
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

pub fn search_english(
    raw: &str,
    options: EnglishSearchOptions,
) -> Result<EnglishSearchResult, EnglishSearchError> {
    search_internal(raw, options, 0)
}

pub fn continue_english(cursor: &EnglishCursor) -> Result<EnglishPage, EnglishSearchError> {
    let result = search_internal(&cursor.raw, cursor.options, cursor.offset)?;
    let concept = result
        .concepts
        .into_iter()
        .find(|concept| concept.query_bytes == cursor.query_bytes)
        .ok_or(EnglishSearchError::StaleCursor)?;
    let entries = concept
        .hits
        .iter()
        .filter_map(|hit| unit_by_runtime_key(hit.runtime_key))
        .collect();
    Ok(EnglishPage {
        query_bytes: concept.query_bytes,
        hits: concept.hits,
        entries,
        next_cursor: concept.next_cursor,
        discovery_truncated: result.discovery_truncated,
    })
}

fn search_internal(
    raw: &str,
    options: EnglishSearchOptions,
    page_offset: usize,
) -> Result<EnglishSearchResult, EnglishSearchError> {
    if options.per_concept_limit == 0 {
        return Err(EnglishSearchError::InvalidPerConceptLimit);
    }
    let metadata = INDEX.metadata()?;
    let maximum = 4_096usize.max(metadata.maximum_phrase_bytes as usize);
    if raw.len() > maximum {
        return Err(EnglishSearchError::InputTooLong {
            bytes: raw.len(),
            maximum,
        });
    }
    let mut discovery = discover(raw, options.completion)?;
    let selected = choose_concepts(&discovery.groups, &discovery.candidates);
    let mut pools = Vec::new();
    for candidate_index in selected {
        let candidate = &discovery.candidates[candidate_index];
        let mut pool = retrieve(candidate)?;
        if pool.len() > CANDIDATE_LIMIT {
            pool.truncate(CANDIDATE_LIMIT);
            discovery.truncated = true;
        }
        pools.push((candidate, pool));
    }
    let mut concepts = Vec::new();
    for (candidate, pool) in &pools {
        let end = page_offset
            .saturating_add(options.per_concept_limit)
            .min(pool.len());
        let hits = if page_offset < pool.len() {
            pool[page_offset..end].iter().map(public_hit).collect()
        } else {
            Vec::new()
        };
        let next_cursor = (end < pool.len()).then(|| EnglishCursor {
            raw: raw.to_owned(),
            options,
            query_bytes: candidate.raw_range.clone(),
            offset: end,
        });
        concepts.push(EnglishConcept {
            query_bytes: candidate.raw_range.clone(),
            hits,
            next_cursor,
        });
    }
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut row = 0;
    while entries.len() < options.limit {
        let mut added = false;
        for concept in &concepts {
            if let Some(hit) = concept.hits.get(row) {
                if seen.insert(hit.runtime_key) {
                    if let Some(unit) = unit_by_runtime_key(hit.runtime_key) {
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
    let has_more = concepts.iter().any(|concept| concept.next_cursor.is_some())
        || concepts
            .iter()
            .flat_map(|concept| &concept.hits)
            .any(|hit| {
                !seen.contains(&hit.runtime_key) && unit_by_runtime_key(hit.runtime_key).is_some()
            });
    Ok(EnglishSearchResult {
        concepts,
        entries,
        discovery_truncated: discovery.truncated,
        has_more,
    })
}

fn public_hit(hit: &RankedHit) -> EnglishHit {
    EnglishHit {
        runtime_key: hit.runtime_key,
        evidence: hit.evidence.clone(),
    }
}

fn discover(raw: &str, completion_mode: CompletionMode) -> Result<Discovery, EnglishSearchError> {
    let maximum_text_tokens = INDEX.metadata()?.maximum_text_tokens as usize;
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
    let mut expanded_states = 0usize;
    let mut occurrence_records = 0usize;
    let mut planned = 0usize;
    let mut truncated = false;
    for view in query_views(raw) {
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
            for length in (1..=segment_group_ids.len()).rev() {
                if length > maximum_text_tokens {
                    continue;
                }
                for local_start in 0..=segment_group_ids.len() - length {
                    let first_id = segment_group_ids[local_start];
                    let last_id = segment_group_ids[local_start + length - 1];
                    let Some(start) = groups.iter().position(|group| group.raw_group == first_id)
                    else {
                        continue;
                    };
                    let Some(last) = groups.iter().position(|group| group.raw_group == last_id)
                    else {
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
                    if planned >= PLAN_LIMIT {
                        truncated = true;
                        break;
                    }
                    planned += 1;
                    for (tokens, flags, grammar_reduced) in forms {
                        let is_final = groups[end - 1].raw_range.end == raw.len();
                        let allow_completion = completion_eligible
                            && is_final
                            && flags == 0
                            && !grammar_reduced
                            && tokens
                                .last()
                                .is_some_and(|token| token.chars().count() >= PREFIX_MINIMUM);
                        let mut plans = discover_form(
                            &tokens,
                            flags,
                            allow_completion,
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
                    }
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
    tokens: &[String],
    flags: u16,
    completion: bool,
    expanded_states: &mut usize,
    occurrence_records: &mut usize,
    truncated: &mut bool,
) -> Result<BTreeSet<Plan>, EnglishSearchError> {
    let mut plans = BTreeSet::new();
    let mut alternatives = Vec::new();
    for token in tokens {
        alternatives.push(token_alternatives(token)?);
    }
    let literal_phrase = tokens.join(" ");
    if let Some(text_key) = INDEX.phrase_key(&literal_phrase)? {
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

    let preceding = selected
        .iter()
        .map(|choice| INDEX.token_string(choice.id))
        .collect::<Result<Vec<_>, _>>()?
        .join(" ");
    let phrase_prefix = format!("{preceding} {typed_prefix}");
    let map = INDEX.phrase_fst()?;
    let mut stream = map.range().ge(&phrase_prefix).into_stream();
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
            .saturating_sub(typed_prefix.chars().count())
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
                .get(family.get() as usize)
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
            .saturating_sub(prefix.chars().count())
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
        let phrase = selected
            .iter()
            .map(|choice| INDEX.token_string(choice.id))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
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
            let text_tokens = text.tokens().collect::<Result<Vec<_>, _>>()?;
            let slice = &text_tokens[start as usize..start as usize + alternatives.len()];
            if !slice
                .iter()
                .zip(&allowed)
                .all(|(token, alternatives)| alternatives.contains(token))
            {
                continue;
            }
            let morphology_changes = slice
                .iter()
                .zip(alternatives)
                .filter(|(matched, choices)| {
                    choices
                        .iter()
                        .find(|choice| choice.id == **matched)
                        .is_some_and(|choice| choice.morphed)
                })
                .count() as u16;
            let added_characters = slice
                .iter()
                .zip(alternatives)
                .filter_map(|(matched, choices)| {
                    choices.iter().find(|choice| choice.id == *matched)
                })
                .map(|choice| choice.added_characters)
                .sum();
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

fn retrieve(candidate: &Candidate) -> Result<Vec<RankedHit>, EnglishSearchError> {
    let mut best = BTreeMap::<u32, RankedHit>::new();
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
                add_ranked(
                    &mut best,
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
                add_ranked(
                    &mut best,
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
    let mut hits = best.into_values().collect::<Vec<_>>();
    hits.sort_by(|left, right| left.rank.cmp(&right.rank));
    Ok(hits)
}

fn add_ranked(
    best: &mut BTreeMap<u32, RankedHit>,
    runtime: RuntimeKey,
    kind: EnglishMatchKind,
    plan: &Plan,
    stored: StoredEvidence,
    surrounding: u32,
) {
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
    let rank = Rank(
        u8::from(kind == EnglishMatchKind::ContainedText),
        u8::from(plan.completion),
        transform_class,
        omitted,
        surrounding,
        plan.added_characters,
        transform_count,
        runtime.get(),
    );
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
    let candidate = RankedHit {
        runtime_key: runtime.get(),
        rank,
        evidence,
    };
    best.entry(runtime.get())
        .and_modify(|old| {
            if candidate.rank < old.rank {
                *old = candidate.clone();
            }
        })
        .or_insert(candidate);
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
    fn zero_per_concept_limit_is_rejected_for_initial_searches_and_continuations() {
        let options = EnglishSearchOptions {
            per_concept_limit: 0,
            ..EnglishSearchOptions::default()
        };
        assert!(matches!(
            search_english("run", options),
            Err(EnglishSearchError::InvalidPerConceptLimit)
        ));

        let cursor = EnglishCursor {
            raw: "run".to_owned(),
            options,
            query_bytes: 0..3,
            offset: 0,
        };
        assert!(matches!(
            continue_english(&cursor),
            Err(EnglishSearchError::InvalidPerConceptLimit)
        ));
        assert_eq!(
            EnglishSearchError::InvalidPerConceptLimit.to_string(),
            "English per-concept limit must be positive"
        );
    }

    #[test]
    fn global_limit_reports_unreturned_unique_entries() {
        let options = EnglishSearchOptions {
            completion: CompletionMode::Disabled,
            limit: 1,
            per_concept_limit: CANDIDATE_LIMIT,
        };
        let result = search_english("watermelon computer", options).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert!(result
            .concepts
            .iter()
            .all(|concept| concept.next_cursor.is_none()));
        assert!(result.has_more);
    }
}
