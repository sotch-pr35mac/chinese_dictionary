#![warn(missing_docs)]
// The consumer keeps the complete schema implementation so generator and reader
// changes can be reviewed side by side, even though only part is used at runtime.
#![allow(dead_code)]

//! Consumer-local normalization and binary reader support for Syng English search.

use fst::Map;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt;
use std::ops::Range;
use unicode_normalization::UnicodeNormalization;

/// Eight-byte identifier at the start of every uncompressed search container.
pub const MAGIC: &[u8; 8] = b"SYNGENG\0";
/// Binary search container version.
pub const SEARCH_FORMAT_VERSION: u32 = 1;
/// Index and query normalization version.
pub const NORMALIZATION_VERSION: u32 = 1;
/// Optional-grammar transformation version.
pub const GRAMMAR_VERSION: u32 = 1;
/// English inflection model version.
pub const MORPHOLOGY_VERSION: u32 = 1;
/// Fixed header byte length.
pub const HEADER_LEN: usize = 48;
/// Fixed section-directory record byte length.
pub const DIRECTORY_ENTRY_LEN: usize = 32;

macro_rules! checked_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u32);

        impl $name {
            /// Constructs an identifier from its stored integer.
            pub const fn new(value: u32) -> Self {
                Self(value)
            }
            /// Returns the stored integer.
            pub const fn get(self) -> u32 {
                self.0
            }
            /// Converts an array index, rejecting values above `u32::MAX`.
            pub fn from_index(value: usize) -> Result<Self, FormatError> {
                Ok(Self(
                    u32::try_from(value).map_err(|_| FormatError::IntegerOverflow)?,
                ))
            }
        }
    };
}

checked_id!(TokenId, "Bundle-local normalized-token identifier.");
checked_id!(TextKey, "Bundle-local normalized search-text identifier.");
checked_id!(
    MorphFamilyId,
    "Bundle-local English lemma and part-of-speech family identifier."
);
checked_id!(RuntimeKey, "Bundle-local lexical-unit identifier.");

/// Derivation flag indicating a top-level semicolon alternative.
pub const DERIVATION_SEMICOLON: u16 = 1 << 0;
/// Derivation flag indicating omitted parenthetical or bracketed text.
pub const DERIVATION_PARENTHETICAL_OMISSION: u16 = 1 << 1;
/// Derivation flag indicating joined hyphen spelling.
pub const DERIVATION_HYPHENS_JOINED: u16 = 1 << 2;
/// Derivation flag indicating separated hyphen spelling.
pub const DERIVATION_HYPHENS_SEPARATED: u16 = 1 << 3;
/// Derivation flag indicating removed apostrophes.
pub const DERIVATION_APOSTROPHE_REMOVED: u16 = 1 << 4;
/// Derivation flag indicating optional grammar reduction.
pub const DERIVATION_GRAMMAR_REDUCED: u16 = 1 << 5;
/// Mask containing every known version-1 derivation flag.
pub const ALL_DERIVATION_FLAGS: u16 = (1 << 6) - 1;

/// Stable section identifiers in the binary directory.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SectionKind {
    /// Token vocabulary finite-state map.
    TokenFst = 1,
    /// Reverse token strings.
    TokenStrings = 2,
    /// Token summary records.
    TokenMeta = 3,
    /// Complete normalized phrase finite-state map.
    PhraseFst = 4,
    /// Search-text summary records.
    TextMeta = 5,
    /// Concatenated search-text token identifiers.
    TextTokens = 6,
    /// Search-text to lexical-unit bindings.
    TextBindings = 7,
    /// Positioned token occurrences.
    TokenOccurrences = 8,
    /// Ranked direct token hits.
    TokenHits = 9,
    /// Morphology surface finite-state map.
    MorphologyFst = 10,
    /// Surface-to-family mappings.
    MorphologyAnalyses = 11,
    /// Family-to-corpus-token mappings.
    MorphologyTokens = 12,
    /// Versioned logical metadata.
    Metadata = 13,
}

impl TryFrom<u32> for SectionKind {
    type Error = FormatError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::TokenFst,
            2 => Self::TokenStrings,
            3 => Self::TokenMeta,
            4 => Self::PhraseFst,
            5 => Self::TextMeta,
            6 => Self::TextTokens,
            7 => Self::TextBindings,
            8 => Self::TokenOccurrences,
            9 => Self::TokenHits,
            10 => Self::MorphologyFst,
            11 => Self::MorphologyAnalyses,
            12 => Self::MorphologyTokens,
            13 => Self::Metadata,
            _ => return Err(FormatError::UnknownSection(value)),
        })
    }
}

/// Section byte encoding.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SectionCodec {
    /// Section-specific version-1 raw encoding.
    RawV1 = 1,
}

/// One owned section supplied to the container writer.
#[derive(Clone, Debug)]
pub struct Section {
    /// Stable section kind.
    pub kind: SectionKind,
    /// Section encoding.
    pub codec: SectionCodec,
    /// Logical number of records represented.
    pub item_count: u32,
    /// Encoded bytes.
    pub bytes: Vec<u8>,
}

/// Parsed section-directory entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectionEntry {
    /// Stable section kind.
    pub kind: SectionKind,
    /// Section encoding.
    pub codec: SectionCodec,
    /// Absolute byte offset in the uncompressed container.
    pub offset: u64,
    /// Byte length.
    pub length: u64,
    /// Logical number of records represented.
    pub item_count: u32,
}

/// Borrowing, structurally validated view of a search container.
#[derive(Debug)]
pub struct EnglishSearchIndex<'a> {
    bytes: &'a [u8],
    lexical_unit_count: u32,
    sections: Vec<SectionEntry>,
}

impl<'a> EnglishSearchIndex<'a> {
    /// Parses and structurally validates a complete uncompressed container.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::Truncated);
        }
        if &bytes[..8] != MAGIC {
            return Err(FormatError::BadMagic);
        }
        for (at, supported, name) in [
            (8, SEARCH_FORMAT_VERSION, "search"),
            (12, NORMALIZATION_VERSION, "normalization"),
            (16, GRAMMAR_VERSION, "grammar"),
            (20, MORPHOLOGY_VERSION, "morphology"),
        ] {
            let found = read_u32(bytes, at)?;
            if found != supported {
                return Err(FormatError::UnsupportedVersion(name, found));
            }
        }
        let lexical_unit_count = read_u32(bytes, 24)?;
        let directory_count = read_u32(bytes, 28)? as usize;
        let directory_offset =
            usize::try_from(read_u64(bytes, 32)?).map_err(|_| FormatError::IntegerOverflow)?;
        if read_u64(bytes, 40)? != 0 {
            return Err(FormatError::ReservedNonzero);
        }
        let directory_len = directory_count
            .checked_mul(DIRECTORY_ENTRY_LEN)
            .ok_or(FormatError::IntegerOverflow)?;
        let directory_end = directory_offset
            .checked_add(directory_len)
            .ok_or(FormatError::IntegerOverflow)?;
        if directory_offset < HEADER_LEN || directory_end > bytes.len() {
            return Err(FormatError::InvalidOffset);
        }
        let mut sections = Vec::with_capacity(directory_count);
        let mut previous_kind = 0;
        let mut ranges = Vec::new();
        for index in 0..directory_count {
            let at = directory_offset + index * DIRECTORY_ENTRY_LEN;
            let raw_kind = read_u32(bytes, at)?;
            if raw_kind <= previous_kind {
                return Err(FormatError::UnsortedOrDuplicateSections);
            }
            previous_kind = raw_kind;
            let kind = SectionKind::try_from(raw_kind)?;
            let codec = match read_u32(bytes, at + 4)? {
                1 => SectionCodec::RawV1,
                value => return Err(FormatError::UnknownCodec(value)),
            };
            let offset = read_u64(bytes, at + 8)?;
            let length = read_u64(bytes, at + 16)?;
            let item_count = read_u32(bytes, at + 24)?;
            if read_u32(bytes, at + 28)? != 0 {
                return Err(FormatError::ReservedNonzero);
            }
            let start = usize::try_from(offset).map_err(|_| FormatError::IntegerOverflow)?;
            let len = usize::try_from(length).map_err(|_| FormatError::IntegerOverflow)?;
            let end = start.checked_add(len).ok_or(FormatError::IntegerOverflow)?;
            if start % 8 != 0 || start < directory_end || end > bytes.len() {
                return Err(FormatError::InvalidOffset);
            }
            ranges.push((start, end));
            sections.push(SectionEntry {
                kind,
                codec,
                offset,
                length,
                item_count,
            });
        }
        ranges.sort_unstable();
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(FormatError::OverlappingSections);
        }
        let required = [
            SectionKind::TokenFst,
            SectionKind::TokenStrings,
            SectionKind::TokenMeta,
            SectionKind::PhraseFst,
            SectionKind::TextMeta,
            SectionKind::TextTokens,
            SectionKind::TextBindings,
            SectionKind::TokenOccurrences,
            SectionKind::TokenHits,
            SectionKind::MorphologyFst,
            SectionKind::MorphologyAnalyses,
            SectionKind::MorphologyTokens,
            SectionKind::Metadata,
        ];
        if required
            .iter()
            .any(|kind| !sections.iter().any(|entry| entry.kind == *kind))
        {
            return Err(FormatError::MissingSection);
        }
        let parsed = Self {
            bytes,
            lexical_unit_count,
            sections,
        };
        Map::new(parsed.section(SectionKind::TokenFst).unwrap())
            .map_err(|_| FormatError::MalformedFst)?;
        Map::new(parsed.section(SectionKind::PhraseFst).unwrap())
            .map_err(|_| FormatError::MalformedFst)?;
        Map::new(parsed.section(SectionKind::MorphologyFst).unwrap())
            .map_err(|_| FormatError::MalformedFst)?;
        Ok(parsed)
    }

    /// Number of lexical units the index was built against.
    pub const fn lexical_unit_count(&self) -> u32 {
        self.lexical_unit_count
    }
    /// Sorted section directory.
    pub fn sections(&self) -> &[SectionEntry] {
        &self.sections
    }
    /// Returns one section's bytes.
    pub fn section(&self, kind: SectionKind) -> Option<&'a [u8]> {
        self.sections
            .iter()
            .find(|entry| entry.kind == kind)
            .map(|entry| {
                let start = entry.offset as usize;
                &self.bytes[start..start + entry.length as usize]
            })
    }

    /// Returns the normalized-token finite-state map.
    pub fn token_fst(&self) -> Result<Map<&'a [u8]>, FormatError> {
        Map::new(self.required_section(SectionKind::TokenFst))
            .map_err(|_| FormatError::MalformedFst)
    }

    /// Returns the complete normalized-phrase finite-state map.
    pub fn phrase_fst(&self) -> Result<Map<&'a [u8]>, FormatError> {
        Map::new(self.required_section(SectionKind::PhraseFst))
            .map_err(|_| FormatError::MalformedFst)
    }

    /// Returns the morphology-surface finite-state map.
    pub fn morphology_fst(&self) -> Result<Map<&'a [u8]>, FormatError> {
        Map::new(self.required_section(SectionKind::MorphologyFst))
            .map_err(|_| FormatError::MalformedFst)
    }

    /// Looks up an exact normalized token.
    pub fn token_id(&self, token: &str) -> Result<Option<TokenId>, FormatError> {
        self.token_fst()?
            .get(token)
            .map(checked_token_id)
            .transpose()
    }

    /// Looks up an exact, ASCII-space-separated normalized phrase.
    pub fn phrase_key(&self, phrase: &str) -> Result<Option<TextKey>, FormatError> {
        self.phrase_fst()?
            .get(phrase)
            .map(checked_text_key)
            .transpose()
    }

    /// Returns the normalized spelling for a token identifier.
    pub fn token_string(&self, token: TokenId) -> Result<&'a str, FormatError> {
        let bytes = self.required_section(SectionKind::TokenStrings);
        if bytes.len() < 16 || read_u32(bytes, 4)? != 0 {
            return Err(FormatError::MalformedSection(SectionKind::TokenStrings));
        }
        let count = read_u32(bytes, 0)?;
        if token.get() >= count || count != self.item_count(SectionKind::TokenStrings) {
            return Err(FormatError::IdentifierOutOfRange);
        }
        let table_len = usize_from_u64(read_u64(bytes, 8)?)?;
        let table_end = checked_add(16, table_len)?;
        let table = bytes
            .get(16..table_end)
            .ok_or(FormatError::MalformedSection(SectionKind::TokenStrings))?;
        let offsets = OffsetTable::parse(table)?;
        if offsets.count != count as usize + 1 {
            return Err(FormatError::MalformedSection(SectionKind::TokenStrings));
        }
        let start = usize_from_u64(offsets.get(token.get() as usize)?)?;
        let end = usize_from_u64(offsets.get(token.get() as usize + 1)?)?;
        std::str::from_utf8(
            bytes
                .get(checked_add(table_end, start)?..checked_add(table_end, end)?)
                .ok_or(FormatError::MalformedSection(SectionKind::TokenStrings))?,
        )
        .map_err(|_| FormatError::InvalidUtf8)
    }

    /// Returns one search text and borrowing iterators over its tokens and bindings.
    pub fn text(&self, key: TextKey) -> Result<SearchText<'a>, FormatError> {
        let meta = TextMeta::parse(
            self.required_section(SectionKind::TextMeta),
            self.item_count(SectionKind::TextMeta),
        )?;
        let index = key.get() as usize;
        if index >= meta.count {
            return Err(FormatError::IdentifierOutOfRange);
        }
        let record_at = checked_add(meta.records_at, checked_mul(index, 8)?)?;
        let token_count = read_u32(meta.bytes, record_at)?;
        let best_flags = read_u16(meta.bytes, record_at + 4)?;
        if token_count == 0
            || best_flags & !ALL_DERIVATION_FLAGS != 0
            || read_u16(meta.bytes, record_at + 6)? != 0
        {
            return Err(FormatError::MalformedSection(SectionKind::TextMeta));
        }
        let token_bytes = slice_offsets(
            self.required_section(SectionKind::TextTokens),
            &meta.token_offsets,
            index,
        )?;
        let binding_bytes = slice_offsets(
            self.required_section(SectionKind::TextBindings),
            &meta.binding_offsets,
            index,
        )?;
        Ok(SearchText {
            key,
            token_count,
            best_flags,
            token_bytes,
            binding_bytes,
        })
    }

    /// Returns one token's occurrence and direct-hit postings.
    pub fn token(&self, id: TokenId) -> Result<SearchToken<'a>, FormatError> {
        let meta = TokenMeta::parse(
            self.required_section(SectionKind::TokenMeta),
            self.item_count(SectionKind::TokenMeta),
        )?;
        let index = id.get() as usize;
        if index >= meta.count {
            return Err(FormatError::IdentifierOutOfRange);
        }
        let occurrence_count = read_u32(meta.bytes, 8 + index * 4)?;
        let document_count = read_u32(meta.bytes, 8 + meta.count * 4 + index * 4)?;
        let hit_count = read_u32(meta.bytes, meta.hit_counts_at + index * 4)?;
        let occurrence_bytes = slice_offsets(
            self.required_section(SectionKind::TokenOccurrences),
            &meta.occurrence_offsets,
            index,
        )?;
        let hit_bytes = slice_offsets(
            self.required_section(SectionKind::TokenHits),
            &meta.hit_offsets,
            index,
        )?;
        Ok(SearchToken {
            id,
            occurrence_count,
            document_count,
            hit_count,
            occurrence_bytes,
            hit_bytes,
        })
    }

    /// Returns morphology analyses for one normalized single-token surface.
    pub fn morphology_analyses(
        &self,
        surface: &str,
    ) -> Result<Option<MorphologyAnalyses<'a>>, FormatError> {
        let Some(offset) = self.morphology_fst()?.get(surface) else {
            return Ok(None);
        };
        let bytes = self.required_section(SectionKind::MorphologyAnalyses);
        let mut cursor = usize_from_u64(offset)?;
        let count = usize_from_u64(read_uleb128(bytes, &mut cursor)?)?;
        Ok(Some(MorphologyAnalyses {
            bytes,
            cursor,
            remaining: count,
            previous: 0,
        }))
    }

    /// Returns a morphology family by its dense identifier.
    pub fn morphology_family(
        &self,
        id: MorphFamilyId,
    ) -> Result<MorphologyFamily<'a>, FormatError> {
        let family_count = self.item_count(SectionKind::MorphologyTokens);
        if id.get() >= family_count {
            return Err(FormatError::IdentifierOutOfRange);
        }
        self.morphology_families()
            .nth(id.get() as usize)
            .expect("family count checked above")
    }

    /// Streams all morphology families once in dense identifier order.
    ///
    /// Consumers that repeatedly resolve family identifiers can collect this
    /// iterator into an ID-indexed cache in one linear pass.
    pub fn morphology_families(&self) -> MorphologyFamilies<'a> {
        MorphologyFamilies {
            bytes: self.required_section(SectionKind::MorphologyTokens),
            cursor: 0,
            next_id: 0,
            remaining: self.item_count(SectionKind::MorphologyTokens),
        }
    }

    /// Decodes the versioned logical metadata record.
    pub fn metadata(&self) -> Result<Metadata<'a>, FormatError> {
        Metadata::parse(self.required_section(SectionKind::Metadata))
    }

    fn required_section(&self, kind: SectionKind) -> &'a [u8] {
        self.section(kind)
            .expect("required sections checked by parse")
    }

    fn item_count(&self, kind: SectionKind) -> u32 {
        self.sections
            .iter()
            .find(|entry| entry.kind == kind)
            .expect("required sections checked by parse")
            .item_count
    }
}

/// One decoded search-text record.
#[derive(Clone, Copy, Debug)]
pub struct SearchText<'a> {
    /// Dense text identifier.
    pub key: TextKey,
    /// Number of normalized tokens in this text.
    pub token_count: u32,
    /// Best stored derivation flags among this text's bindings.
    pub best_flags: u16,
    token_bytes: &'a [u8],
    binding_bytes: &'a [u8],
}

impl<'a> SearchText<'a> {
    /// Streams the text's token identifiers in phrase order.
    pub fn tokens(&self) -> TextTokens<'a> {
        TextTokens {
            bytes: self.token_bytes,
            cursor: 0,
            remaining: self.token_count as usize,
        }
    }

    /// Streams this text's lexical-unit bindings in stored order.
    pub fn bindings(&self) -> TextBindings<'a> {
        TextBindings {
            bytes: self.binding_bytes,
            cursor: 0,
            previous: 0,
        }
    }
}

/// Iterator over a search text's token identifiers.
pub struct TextTokens<'a> {
    bytes: &'a [u8],
    cursor: usize,
    remaining: usize,
}

impl Iterator for TextTokens<'_> {
    type Item = Result<TokenId, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(read_uleb128(self.bytes, &mut self.cursor).and_then(checked_token_id))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for TextTokens<'_> {}

/// One search-text binding to a lexical unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextBinding {
    /// Bundle-local lexical-unit key.
    pub runtime_key: RuntimeKey,
    /// Stored text derivations supporting this binding.
    pub derivation_flags: u16,
}

/// Iterator over a search text's bindings.
pub struct TextBindings<'a> {
    bytes: &'a [u8],
    cursor: usize,
    previous: u32,
}

impl Iterator for TextBindings<'_> {
    type Item = Result<TextBinding, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor == self.bytes.len() {
            return None;
        }
        Some((|| {
            let delta = checked_u32(read_uleb128(self.bytes, &mut self.cursor)?)?;
            self.previous = self
                .previous
                .checked_add(delta)
                .ok_or(FormatError::IntegerOverflow)?;
            let flags = checked_u16(read_uleb128(self.bytes, &mut self.cursor)?)?;
            if flags & !ALL_DERIVATION_FLAGS != 0 {
                return Err(FormatError::UnknownDerivationFlags(flags));
            }
            Ok(TextBinding {
                runtime_key: RuntimeKey::new(self.previous),
                derivation_flags: flags,
            })
        })())
    }
}

/// One token and its posting-list summaries.
#[derive(Clone, Copy, Debug)]
pub struct SearchToken<'a> {
    /// Dense token identifier.
    pub id: TokenId,
    /// Total number of positioned occurrences.
    pub occurrence_count: u32,
    /// Number of distinct search texts containing the token.
    pub document_count: u32,
    /// Number of direct lexical-unit hits.
    pub hit_count: u32,
    occurrence_bytes: &'a [u8],
    hit_bytes: &'a [u8],
}

impl<'a> SearchToken<'a> {
    /// Streams every `(TextKey, token_position)` occurrence.
    pub fn occurrences(&self) -> Result<TokenOccurrences<'a>, FormatError> {
        TokenOccurrences::new(self.occurrence_bytes)
    }

    /// Streams ranked direct lexical-unit hits.
    pub fn direct_hits(&self) -> DirectTokenHits<'a> {
        DirectTokenHits::new(self.hit_bytes)
    }
}

/// One positioned occurrence of a token in a search text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenOccurrence {
    /// Search text containing the token.
    pub text_key: TextKey,
    /// Zero-based token position within the text.
    pub position: u32,
}

/// Borrowing iterator over blocked token occurrences.
pub struct TokenOccurrences<'a> {
    part: &'a [u8],
    payloads_at: usize,
    block_count: usize,
    block_index: usize,
    remaining_in_block: usize,
    payload_cursor: usize,
    payload_end: usize,
    text: u32,
}

impl<'a> TokenOccurrences<'a> {
    fn new(part: &'a [u8]) -> Result<Self, FormatError> {
        let block_count = read_u32(part, 0)? as usize;
        let payloads_at = checked_add(4, checked_mul(block_count, 16)?)?;
        if payloads_at > part.len() {
            return Err(FormatError::MalformedSection(SectionKind::TokenOccurrences));
        }
        Ok(Self {
            part,
            payloads_at,
            block_count,
            block_index: 0,
            remaining_in_block: 0,
            payload_cursor: 0,
            payload_end: 0,
            text: 0,
        })
    }

    fn start_block(&mut self) -> Result<bool, FormatError> {
        if self.block_index == self.block_count {
            return Ok(false);
        }
        let at = 4 + self.block_index * 16;
        self.text = read_u32(self.part, at)?;
        self.remaining_in_block = read_u32(self.part, at + 4)? as usize;
        if self.remaining_in_block == 0 || self.remaining_in_block > 128 {
            return Err(FormatError::MalformedSection(SectionKind::TokenOccurrences));
        }
        let offset = usize_from_u64(read_u64(self.part, at + 8)?)?;
        self.payload_cursor = checked_add(self.payloads_at, offset)?;
        let payload_len = usize_from_u64(read_uleb128(self.part, &mut self.payload_cursor)?)?;
        self.payload_end = checked_add(self.payload_cursor, payload_len)?;
        if self.payload_end > self.part.len() {
            return Err(FormatError::Truncated);
        }
        self.block_index += 1;
        Ok(true)
    }
}

impl Iterator for TokenOccurrences<'_> {
    type Item = Result<TokenOccurrence, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining_in_block == 0 {
            match self.start_block() {
                Ok(true) => {}
                Ok(false) => return None,
                Err(error) => return Some(Err(error)),
            }
        }
        self.remaining_in_block -= 1;
        Some((|| {
            let delta = checked_u32(read_uleb128(self.part, &mut self.payload_cursor)?)?;
            self.text = self
                .text
                .checked_add(delta)
                .ok_or(FormatError::IntegerOverflow)?;
            let position = checked_u32(read_uleb128(self.part, &mut self.payload_cursor)?)?;
            if self.payload_cursor > self.payload_end
                || (self.remaining_in_block == 0 && self.payload_cursor != self.payload_end)
            {
                return Err(FormatError::MalformedSection(SectionKind::TokenOccurrences));
            }
            Ok(TokenOccurrence {
                text_key: TextKey::new(self.text),
                position,
            })
        })())
    }
}

/// Build-time evidence encoded in a ranked direct-hit bucket.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DirectHitRank {
    /// The token occurs inside a multi-token text rather than as the whole text.
    pub containment: bool,
    /// The stored evidence uses a spelling or grammar derivation.
    pub derived: bool,
    /// The stored evidence omits parenthetical text.
    pub parenthetical_omission: bool,
    /// Number of tokens in the containing search text.
    pub text_token_count: u32,
    /// Number of stored transformations.
    pub transformation_count: u16,
}

/// One ranked direct token hit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectTokenHit {
    /// Bundle-local lexical-unit key.
    pub runtime_key: RuntimeKey,
    /// Build-time evidence rank. Lower values sort first.
    pub rank: DirectHitRank,
}

/// Borrowing iterator over direct token hits in rank order.
pub struct DirectTokenHits<'a> {
    bytes: &'a [u8],
    cursor: usize,
    buckets_remaining: usize,
    keys_remaining: usize,
    runtime: u32,
    rank: DirectHitRank,
    initial_error: Option<FormatError>,
}

impl<'a> DirectTokenHits<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        let mut cursor = 0;
        let (buckets_remaining, initial_error) =
            match read_uleb128(bytes, &mut cursor).and_then(usize_from_u64) {
                Ok(value) => (value, None),
                Err(error) => (0, Some(error)),
            };
        Self {
            bytes,
            cursor,
            buckets_remaining,
            keys_remaining: 0,
            runtime: 0,
            rank: DirectHitRank {
                containment: false,
                derived: false,
                parenthetical_omission: false,
                text_token_count: 0,
                transformation_count: 0,
            },
            initial_error,
        }
    }
}

impl Iterator for DirectTokenHits<'_> {
    type Item = Result<DirectTokenHit, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(error) = self.initial_error.take() {
            return Some(Err(error));
        }
        if self.keys_remaining == 0 {
            if self.buckets_remaining == 0 {
                return None;
            }
            let result = (|| {
                let packed = *self.bytes.get(self.cursor).ok_or(FormatError::Truncated)?;
                self.cursor += 1;
                if packed & !0b111 != 0 {
                    return Err(FormatError::MalformedSection(SectionKind::TokenHits));
                }
                self.rank = DirectHitRank {
                    containment: packed & 0b100 != 0,
                    derived: packed & 0b010 != 0,
                    parenthetical_omission: packed & 0b001 != 0,
                    text_token_count: checked_u32(read_uleb128(self.bytes, &mut self.cursor)?)?,
                    transformation_count: checked_u16(read_uleb128(self.bytes, &mut self.cursor)?)?,
                };
                self.keys_remaining = usize_from_u64(read_uleb128(self.bytes, &mut self.cursor)?)?;
                self.runtime = 0;
                self.buckets_remaining -= 1;
                Ok(())
            })();
            if let Err(error) = result {
                self.buckets_remaining = 0;
                return Some(Err(error));
            }
            if self.keys_remaining == 0 {
                return Some(Err(FormatError::MalformedSection(SectionKind::TokenHits)));
            }
        }
        self.keys_remaining -= 1;
        Some((|| {
            let delta = checked_u32(read_uleb128(self.bytes, &mut self.cursor)?)?;
            self.runtime = self
                .runtime
                .checked_add(delta)
                .ok_or(FormatError::IntegerOverflow)?;
            Ok(DirectTokenHit {
                runtime_key: RuntimeKey::new(self.runtime),
                rank: self.rank,
            })
        })())
    }
}

/// English morphology part of speech.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MorphologyPartOfSpeech {
    /// Noun inflection family.
    Noun,
    /// Verb inflection family.
    Verb,
}

/// Borrowing iterator over all morphology families in dense identifier order.
pub struct MorphologyFamilies<'a> {
    bytes: &'a [u8],
    cursor: usize,
    next_id: u32,
    remaining: u32,
}

impl<'a> Iterator for MorphologyFamilies<'a> {
    type Item = Result<MorphologyFamily<'a>, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let id = MorphFamilyId::new(self.next_id);
        self.next_id += 1;
        Some((|| {
            let part_of_speech = match *self.bytes.get(self.cursor).ok_or(FormatError::Truncated)? {
                1 => MorphologyPartOfSpeech::Noun,
                2 => MorphologyPartOfSpeech::Verb,
                _ => return Err(FormatError::UnknownPartOfSpeech),
            };
            self.cursor += 1;
            let lemma_len = usize_from_u64(read_uleb128(self.bytes, &mut self.cursor)?)?;
            let lemma_end = checked_add(self.cursor, lemma_len)?;
            let lemma = std::str::from_utf8(
                self.bytes
                    .get(self.cursor..lemma_end)
                    .ok_or(FormatError::Truncated)?,
            )
            .map_err(|_| FormatError::InvalidUtf8)?;
            self.cursor = lemma_end;
            let token_count = usize_from_u64(read_uleb128(self.bytes, &mut self.cursor)?)?;
            let token_start = self.cursor;
            for _ in 0..token_count {
                read_uleb128(self.bytes, &mut self.cursor)?;
            }
            Ok(MorphologyFamily {
                id,
                part_of_speech,
                lemma,
                token_bytes: &self.bytes[token_start..self.cursor],
                token_count,
            })
        })())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for MorphologyFamilies<'_> {}

/// Iterator over morphology families analyzing a query surface.
pub struct MorphologyAnalyses<'a> {
    bytes: &'a [u8],
    cursor: usize,
    remaining: usize,
    previous: u32,
}

impl Iterator for MorphologyAnalyses<'_> {
    type Item = Result<MorphFamilyId, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some((|| {
            let delta = checked_u32(read_uleb128(self.bytes, &mut self.cursor)?)?;
            self.previous = self
                .previous
                .checked_add(delta)
                .ok_or(FormatError::IntegerOverflow)?;
            Ok(MorphFamilyId::new(self.previous))
        })())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for MorphologyAnalyses<'_> {}

/// One morphology family and its corpus token identifiers.
#[derive(Clone, Copy, Debug)]
pub struct MorphologyFamily<'a> {
    /// Dense morphology-family identifier.
    pub id: MorphFamilyId,
    /// Noun or verb inflection family.
    pub part_of_speech: MorphologyPartOfSpeech,
    /// Normalized lemma.
    pub lemma: &'a str,
    token_bytes: &'a [u8],
    token_count: usize,
}

impl<'a> MorphologyFamily<'a> {
    /// Streams corpus token identifiers belonging to this family.
    pub fn corpus_tokens(&self) -> MorphologyTokens<'a> {
        MorphologyTokens {
            bytes: self.token_bytes,
            cursor: 0,
            remaining: self.token_count,
            previous: 0,
        }
    }
}

/// Iterator over the corpus tokens in a morphology family.
pub struct MorphologyTokens<'a> {
    bytes: &'a [u8],
    cursor: usize,
    remaining: usize,
    previous: u32,
}

impl Iterator for MorphologyTokens<'_> {
    type Item = Result<TokenId, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some((|| {
            let delta = checked_u32(read_uleb128(self.bytes, &mut self.cursor)?)?;
            self.previous = self
                .previous
                .checked_add(delta)
                .ok_or(FormatError::IntegerOverflow)?;
            Ok(TokenId::new(self.previous))
        })())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for MorphologyTokens<'_> {}

/// Logical counts and provenance stored with an English search index.
#[derive(Clone, Copy, Debug)]
pub struct Metadata<'a> {
    /// Search format version repeated in metadata.
    pub search_format_version: u32,
    /// Normalization version repeated in metadata.
    pub normalization_version: u32,
    /// Optional-grammar version repeated in metadata.
    pub grammar_version: u32,
    /// Morphology version repeated in metadata.
    pub morphology_version: u32,
    /// Number of normalized corpus tokens.
    pub token_count: u32,
    /// Number of deduplicated search texts.
    pub text_count: u32,
    /// Number of text-to-lexical-unit bindings.
    pub binding_count: u32,
    /// Number of positioned token occurrences.
    pub occurrence_count: u32,
    /// Number of direct token hits.
    pub direct_hit_count: u32,
    /// Number of retained morphology families.
    pub morphology_family_count: u32,
    /// Number of supported morphology surfaces.
    pub morphology_surface_count: u32,
    /// Largest token count of any search text.
    pub maximum_text_tokens: u32,
    /// Largest normalized phrase byte length.
    pub maximum_phrase_bytes: u32,
    /// WordNet input revision.
    pub wordnet_revision: &'a str,
    /// Checked-in morphology override revision.
    pub override_revision: &'a str,
    section_sizes: &'a [u8],
    section_count: usize,
}

impl<'a> Metadata<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        let mut cursor = 0;
        let mut values = [0_u32; 13];
        for value in &mut values {
            *value = read_u32(bytes, cursor)?;
            cursor += 4;
        }
        let wordnet_revision = read_string(bytes, &mut cursor)?;
        let override_revision = read_string(bytes, &mut cursor)?;
        let section_count = read_u32(bytes, cursor)? as usize;
        cursor += 4;
        let section_len = checked_mul(section_count, 12)?;
        let end = checked_add(cursor, section_len)?;
        if end != bytes.len() {
            return Err(FormatError::MalformedSection(SectionKind::Metadata));
        }
        Ok(Self {
            search_format_version: values[0],
            normalization_version: values[1],
            grammar_version: values[2],
            morphology_version: values[3],
            token_count: values[4],
            text_count: values[5],
            binding_count: values[6],
            occurrence_count: values[7],
            direct_hit_count: values[8],
            morphology_family_count: values[9],
            morphology_surface_count: values[10],
            maximum_text_tokens: values[11],
            maximum_phrase_bytes: values[12],
            wordnet_revision,
            override_revision,
            section_sizes: &bytes[cursor..end],
            section_count,
        })
    }

    /// Streams the raw byte length recorded for every section.
    pub fn section_sizes(&self) -> MetadataSectionSizes<'a> {
        MetadataSectionSizes {
            bytes: self.section_sizes,
            index: 0,
            count: self.section_count,
        }
    }
}

/// One metadata section-size record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataSectionSize {
    /// Section kind.
    pub kind: SectionKind,
    /// Raw section length in bytes.
    pub bytes: u64,
}

/// Iterator over metadata section-size records.
pub struct MetadataSectionSizes<'a> {
    bytes: &'a [u8],
    index: usize,
    count: usize,
}

impl Iterator for MetadataSectionSizes<'_> {
    type Item = Result<MetadataSectionSize, FormatError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.count {
            return None;
        }
        let at = self.index * 12;
        self.index += 1;
        Some((|| {
            Ok(MetadataSectionSize {
                kind: SectionKind::try_from(read_u32(self.bytes, at)?)?,
                bytes: read_u64(self.bytes, at + 4)?,
            })
        })())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.count - self.index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for MetadataSectionSizes<'_> {}

struct TextMeta<'a> {
    bytes: &'a [u8],
    count: usize,
    token_offsets: OffsetTable<'a>,
    binding_offsets: OffsetTable<'a>,
    records_at: usize,
}

impl<'a> TextMeta<'a> {
    fn parse(bytes: &'a [u8], expected_count: u32) -> Result<Self, FormatError> {
        if bytes.len() < 24 || read_u32(bytes, 4)? != 0 {
            return Err(FormatError::MalformedSection(SectionKind::TextMeta));
        }
        let count = read_u32(bytes, 0)? as usize;
        if count != expected_count as usize {
            return Err(FormatError::MalformedSection(SectionKind::TextMeta));
        }
        let token_len = usize_from_u64(read_u64(bytes, 8)?)?;
        let binding_len = usize_from_u64(read_u64(bytes, 16)?)?;
        let token_end = checked_add(24, token_len)?;
        let binding_end = checked_add(token_end, binding_len)?;
        let expected_end = checked_add(binding_end, checked_mul(count, 8)?)?;
        if expected_end != bytes.len() {
            return Err(FormatError::MalformedSection(SectionKind::TextMeta));
        }
        let token_offsets = OffsetTable::parse(&bytes[24..token_end])?;
        let binding_offsets = OffsetTable::parse(&bytes[token_end..binding_end])?;
        if token_offsets.count != count + 1 || binding_offsets.count != count + 1 {
            return Err(FormatError::MalformedSection(SectionKind::TextMeta));
        }
        Ok(Self {
            bytes,
            count,
            token_offsets,
            binding_offsets,
            records_at: binding_end,
        })
    }
}

struct TokenMeta<'a> {
    bytes: &'a [u8],
    count: usize,
    occurrence_offsets: OffsetTable<'a>,
    hit_offsets: OffsetTable<'a>,
    hit_counts_at: usize,
}

impl<'a> TokenMeta<'a> {
    fn parse(bytes: &'a [u8], expected_count: u32) -> Result<Self, FormatError> {
        if bytes.len() < 24 || read_u32(bytes, 4)? != 0 {
            return Err(FormatError::MalformedSection(SectionKind::TokenMeta));
        }
        let count = read_u32(bytes, 0)? as usize;
        if count != expected_count as usize {
            return Err(FormatError::MalformedSection(SectionKind::TokenMeta));
        }
        let table_lengths_at = checked_add(8, checked_mul(count, 8)?)?;
        let occurrence_len = usize_from_u64(read_u64(bytes, table_lengths_at)?)?;
        let hit_len = usize_from_u64(read_u64(bytes, table_lengths_at + 8)?)?;
        let occurrence_at = table_lengths_at + 16;
        let hit_at = checked_add(occurrence_at, occurrence_len)?;
        let hit_counts_at = checked_add(hit_at, hit_len)?;
        if checked_add(hit_counts_at, checked_mul(count, 4)?)? != bytes.len() {
            return Err(FormatError::MalformedSection(SectionKind::TokenMeta));
        }
        let occurrence_offsets = OffsetTable::parse(&bytes[occurrence_at..hit_at])?;
        let hit_offsets = OffsetTable::parse(&bytes[hit_at..hit_counts_at])?;
        if occurrence_offsets.count != count + 1 || hit_offsets.count != count + 1 {
            return Err(FormatError::MalformedSection(SectionKind::TokenMeta));
        }
        Ok(Self {
            bytes,
            count,
            occurrence_offsets,
            hit_offsets,
            hit_counts_at,
        })
    }
}

#[derive(Clone, Copy)]
struct OffsetTable<'a> {
    bytes: &'a [u8],
    count: usize,
    blocks: usize,
    payload_at: usize,
}

impl<'a> OffsetTable<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        let count = read_u32(bytes, 0)? as usize;
        let blocks = read_u32(bytes, 4)? as usize;
        if blocks != count.div_ceil(128) {
            return Err(FormatError::MalformedOffsetTable);
        }
        let payload_at = checked_add(8, checked_mul(blocks, 24)?)?;
        if payload_at > bytes.len() {
            return Err(FormatError::MalformedOffsetTable);
        }
        Ok(Self {
            bytes,
            count,
            blocks,
            payload_at,
        })
    }

    fn get(&self, index: usize) -> Result<u64, FormatError> {
        if index >= self.count {
            return Err(FormatError::IdentifierOutOfRange);
        }
        let block = index / 128;
        if block >= self.blocks {
            return Err(FormatError::MalformedOffsetTable);
        }
        let within = index % 128;
        let at = 8 + block * 24;
        let first_index = read_u32(self.bytes, at)? as usize;
        let values = read_u32(self.bytes, at + 4)? as usize;
        let mut value = read_u64(self.bytes, at + 8)?;
        let payload_offset = usize_from_u64(read_u64(self.bytes, at + 16)?)?;
        if first_index != block * 128 || values == 0 || values > 128 || within >= values {
            return Err(FormatError::MalformedOffsetTable);
        }
        let mut cursor = checked_add(self.payload_at, payload_offset)?;
        for _ in 0..within {
            value = value
                .checked_add(read_uleb128(self.bytes, &mut cursor)?)
                .ok_or(FormatError::IntegerOverflow)?;
        }
        Ok(value)
    }
}

fn slice_offsets<'a>(
    bytes: &'a [u8],
    offsets: &OffsetTable<'_>,
    index: usize,
) -> Result<&'a [u8], FormatError> {
    let start = usize_from_u64(offsets.get(index)?)?;
    let end = usize_from_u64(offsets.get(index + 1)?)?;
    bytes.get(start..end).ok_or(FormatError::InvalidOffset)
}

fn read_string<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a str, FormatError> {
    let len = read_u32(bytes, *cursor)? as usize;
    *cursor = checked_add(*cursor, 4)?;
    let end = checked_add(*cursor, len)?;
    let value = std::str::from_utf8(bytes.get(*cursor..end).ok_or(FormatError::Truncated)?)
        .map_err(|_| FormatError::InvalidUtf8)?;
    *cursor = end;
    Ok(value)
}

fn checked_token_id(value: u64) -> Result<TokenId, FormatError> {
    Ok(TokenId::new(checked_u32(value)?))
}

fn checked_text_key(value: u64) -> Result<TextKey, FormatError> {
    Ok(TextKey::new(checked_u32(value)?))
}

fn checked_u32(value: u64) -> Result<u32, FormatError> {
    u32::try_from(value).map_err(|_| FormatError::IntegerOverflow)
}

fn checked_u16(value: u64) -> Result<u16, FormatError> {
    u16::try_from(value).map_err(|_| FormatError::IntegerOverflow)
}

fn usize_from_u64(value: u64) -> Result<usize, FormatError> {
    usize::try_from(value).map_err(|_| FormatError::IntegerOverflow)
}

fn checked_add(left: usize, right: usize) -> Result<usize, FormatError> {
    left.checked_add(right).ok_or(FormatError::IntegerOverflow)
}

fn checked_mul(left: usize, right: usize) -> Result<usize, FormatError> {
    left.checked_mul(right).ok_or(FormatError::IntegerOverflow)
}

/// Writes one deterministic uncompressed container from pre-encoded sections.
pub fn write_container(
    lexical_unit_count: u32,
    mut sections: Vec<Section>,
) -> Result<Vec<u8>, FormatError> {
    sections.sort_by_key(|section| section.kind);
    if sections.windows(2).any(|pair| pair[0].kind == pair[1].kind) {
        return Err(FormatError::UnsortedOrDuplicateSections);
    }
    let count = u32::try_from(sections.len()).map_err(|_| FormatError::IntegerOverflow)?;
    let directory_end = HEADER_LEN
        .checked_add(
            sections
                .len()
                .checked_mul(DIRECTORY_ENTRY_LEN)
                .ok_or(FormatError::IntegerOverflow)?,
        )
        .ok_or(FormatError::IntegerOverflow)?;
    let first_section = align8(directory_end);
    let mut offsets = Vec::with_capacity(sections.len());
    let mut cursor = first_section;
    for section in &sections {
        cursor = align8(cursor);
        offsets.push(cursor);
        cursor = cursor
            .checked_add(section.bytes.len())
            .ok_or(FormatError::IntegerOverflow)?;
    }
    let mut output = vec![0; cursor];
    output[..8].copy_from_slice(MAGIC);
    put_u32(&mut output, 8, SEARCH_FORMAT_VERSION);
    put_u32(&mut output, 12, NORMALIZATION_VERSION);
    put_u32(&mut output, 16, GRAMMAR_VERSION);
    put_u32(&mut output, 20, MORPHOLOGY_VERSION);
    put_u32(&mut output, 24, lexical_unit_count);
    put_u32(&mut output, 28, count);
    put_u64(&mut output, 32, HEADER_LEN as u64);
    for (index, (section, offset)) in sections.iter().zip(offsets).enumerate() {
        let at = HEADER_LEN + index * DIRECTORY_ENTRY_LEN;
        put_u32(&mut output, at, section.kind as u32);
        put_u32(&mut output, at + 4, section.codec as u32);
        put_u64(&mut output, at + 8, offset as u64);
        put_u64(&mut output, at + 16, section.bytes.len() as u64);
        put_u32(&mut output, at + 24, section.item_count);
        output[offset..offset + section.bytes.len()].copy_from_slice(&section.bytes);
    }
    EnglishSearchIndex::parse(&output)?;
    Ok(output)
}

/// One normalized query token with its original UTF-8 byte coverage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryToken {
    /// Normalized spelling.
    pub text: String,
    /// Byte range in the original query.
    pub raw_range: Range<usize>,
    /// Raw-token group; aliases from one hyphenated token share this value.
    pub raw_group: u32,
}

/// A phrase segment that cannot match across hard punctuation boundaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuerySegment {
    /// Literal normalized tokens.
    pub tokens: Vec<QueryToken>,
}

/// One uniformly transformed view of all query phrase segments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryView {
    /// Hard-boundary-delimited query segments.
    pub segments: Vec<QuerySegment>,
    /// Spelling derivations applied to this view.
    pub derivation_flags: u16,
}

/// Produces literal and uniform spelling-alias query views with raw coverage intact.
pub fn query_views(raw: &str) -> Vec<QueryView> {
    let literal = tokenize_query(raw);
    query_views_from_literal(&literal)
}

/// Produces query views from an already-tokenized literal query.
///
/// This avoids repeating Unicode normalization when a caller also needs the
/// literal tokens for byte-range planning.
pub(crate) fn query_views_from_literal(literal: &[QuerySegment]) -> Vec<QueryView> {
    let has_apostrophe = literal
        .iter()
        .flat_map(|segment| &segment.tokens)
        .any(|token| token.text.contains('\''));
    let has_hyphen = literal
        .iter()
        .flat_map(|segment| &segment.tokens)
        .any(|token| token.text.contains('-'));
    let mut modes = BTreeSet::from([(false, 0_u8)]);
    if has_apostrophe {
        modes.insert((true, 0));
    }
    if has_hyphen {
        modes.insert((false, 1));
        modes.insert((false, 2));
    }
    if has_apostrophe && has_hyphen {
        modes.insert((true, 1));
        modes.insert((true, 2));
    }
    modes
        .into_iter()
        .map(|(remove_apostrophe, hyphen_mode)| {
            let mut flags = if remove_apostrophe {
                DERIVATION_APOSTROPHE_REMOVED
            } else {
                0
            };
            flags |= match hyphen_mode {
                1 => DERIVATION_HYPHENS_JOINED,
                2 => DERIVATION_HYPHENS_SEPARATED,
                _ => 0,
            };
            let segments = literal
                .iter()
                .map(|segment| {
                    let tokens = segment
                        .tokens
                        .iter()
                        .flat_map(|token| {
                            let text = if remove_apostrophe {
                                token.text.replace('\'', "")
                            } else {
                                token.text.clone()
                            };
                            let forms = match hyphen_mode {
                                1 => vec![text.replace('-', "")],
                                2 => text
                                    .split('-')
                                    .filter(|part| !part.is_empty())
                                    .map(str::to_owned)
                                    .collect(),
                                _ => vec![text],
                            };
                            forms
                                .into_iter()
                                .map(|text| QueryToken {
                                    text,
                                    raw_range: token.raw_range.clone(),
                                    raw_group: token.raw_group,
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect();
                    QuerySegment { tokens }
                })
                .collect();
            QueryView {
                segments,
                derivation_flags: flags,
            }
        })
        .collect()
}

/// Tokenizes a query while retaining hard boundaries and raw UTF-8 ranges.
pub fn tokenize_query(raw: &str) -> Vec<QuerySegment> {
    let mut segments = Vec::new();
    let mut tokens = Vec::new();
    let mut token_start = None;
    let mut group = 0_u32;
    let finish =
        |end: usize, start: &mut Option<usize>, tokens: &mut Vec<QueryToken>, group: &mut u32| {
            if let Some(begin) = start.take() {
                let normalized = normalize_text_cow(&raw[begin..end]);
                if normalized.contains(' ') {
                    for text in normalized.split(' ').filter(|text| !text.is_empty()) {
                        tokens.push(QueryToken {
                            text: text.to_owned(),
                            raw_range: begin..end,
                            raw_group: *group,
                        });
                    }
                } else if !normalized.is_empty() {
                    tokens.push(QueryToken {
                        text: normalized.into_owned(),
                        raw_range: begin..end,
                        raw_group: *group,
                    });
                }
                *group = group.saturating_add(1);
            }
        };
    let mut chars = raw.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        let end = chars.peek().map_or(raw.len(), |value| value.0);
        if is_hard_boundary(ch) {
            finish(at, &mut token_start, &mut tokens, &mut group);
            if !tokens.is_empty() {
                segments.push(QuerySegment {
                    tokens: std::mem::take(&mut tokens),
                });
            }
        } else if is_base_character(ch)
            || (is_combining_mark(ch) && token_start.is_some())
            || ((ch == '\'' || ch == '’' || ch == '-')
                && token_start.is_some()
                && chars
                    .peek()
                    .is_some_and(|(_, next)| is_base_character(*next)))
        {
            token_start.get_or_insert(at);
        } else {
            finish(at, &mut token_start, &mut tokens, &mut group);
        }
        if chars.peek().is_none() {
            finish(end, &mut token_start, &mut tokens, &mut group);
        }
    }
    if !tokens.is_empty() {
        segments.push(QuerySegment { tokens });
    }
    segments
}

/// Normalizes prose into ASCII-space-separated lexical tokens.
pub fn normalize_text(value: &str) -> String {
    normalize_text_cow(value).into_owned()
}

/// Normalizes prose while borrowing already-normalized ASCII input.
pub fn normalize_text_cow(value: &str) -> Cow<'_, str> {
    let bytes = value.as_bytes();
    let already_normalized = bytes.iter().enumerate().all(|(index, byte)| match byte {
        b'a'..=b'z' | b'0'..=b'9' => true,
        b' ' => {
            index > 0
                && index + 1 < bytes.len()
                && bytes[index - 1] != b' '
                && bytes[index + 1] != b' '
        }
        b'\'' | b'-' => {
            index > 0
                && index + 1 < bytes.len()
                && bytes[index - 1].is_ascii_alphanumeric()
                && bytes[index + 1].is_ascii_alphanumeric()
        }
        _ => false,
    });
    if already_normalized {
        return Cow::Borrowed(value);
    }
    let folded = value
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let chars: Vec<char> = folded.nfc().collect();
    let mut result = String::new();
    let mut previous_space = true;
    for (index, &ch) in chars.iter().enumerate() {
        let attached = is_base_character(ch) || (is_combining_mark(ch) && !previous_space);
        let internal = (ch == '\'' || ch == '’' || ch == '-')
            && index > 0
            && index + 1 < chars.len()
            && (is_base_character(chars[index - 1]) || is_combining_mark(chars[index - 1]))
            && is_base_character(chars[index + 1]);
        if attached || internal {
            if ch == '’' {
                result.push('\'');
            } else {
                result.push(ch);
            }
            previous_space = false;
        } else if !previous_space && !result.is_empty() {
            result.push(' ');
            previous_space = true;
        }
    }
    while result.ends_with(' ') {
        result.pop();
    }
    Cow::Owned(result)
}

/// Returns the literal spelling plus deterministic apostrophe and hyphen aliases.
pub fn spelling_views(normalized: &str) -> Vec<(String, u16)> {
    let mut views = BTreeSet::from([(normalized.to_owned(), 0_u16)]);
    if normalized.contains('\'') {
        views.insert((normalized.replace('\'', ""), DERIVATION_APOSTROPHE_REMOVED));
    }
    if normalized.contains('-') {
        views.insert((normalized.replace('-', ""), DERIVATION_HYPHENS_JOINED));
        views.insert((
            normalized
                .replace('-', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            DERIVATION_HYPHENS_SEPARATED,
        ));
    }
    views.into_iter().collect()
}

/// Applies the single optional-grammar reduction view to normalized tokens.
pub fn reduce_optional_grammar(tokens: &[&str]) -> Option<Vec<String>> {
    if tokens.len() < 2 {
        return None;
    }
    let mut reduced = tokens
        .iter()
        .copied()
        .filter(|token| !matches!(*token, "a" | "an" | "the"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if reduced.len() > 1 && reduced.first().is_some_and(|token| token == "to") {
        reduced.remove(0);
    }
    if reduced.len() > 1
        && reduced.first().is_some_and(|token| {
            matches!(
                token.as_str(),
                "be" | "am" | "is" | "are" | "was" | "were" | "been" | "being"
            )
        })
    {
        reduced.remove(0);
    }
    if reduced.is_empty()
        || reduced
            .iter()
            .map(String::as_str)
            .eq(tokens.iter().copied())
    {
        None
    } else {
        Some(reduced)
    }
}

/// Encodes an unsigned integer using canonical LEB128.
pub fn write_uleb128(mut value: u64, output: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Decodes one canonical unsigned LEB128 integer.
pub fn read_uleb128(bytes: &[u8], cursor: &mut usize) -> Result<u64, FormatError> {
    let start = *cursor;
    let mut result = 0_u64;
    for shift in (0..=63).step_by(7) {
        let byte = *bytes.get(*cursor).ok_or(FormatError::Truncated)?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(FormatError::MalformedLeb128);
        }
        result |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            let mut canonical = Vec::new();
            write_uleb128(result, &mut canonical);
            if canonical.len() != *cursor - start {
                return Err(FormatError::MalformedLeb128);
            }
            return Ok(result);
        }
    }
    Err(FormatError::MalformedLeb128)
}

fn is_base_character(ch: char) -> bool {
    ch.is_alphanumeric()
}
fn is_combining_mark(ch: char) -> bool {
    unicode_normalization::char::is_combining_mark(ch)
}
fn is_hard_boundary(ch: char) -> bool {
    matches!(
        ch,
        ',' | ';'
            | ':'
            | '.'
            | '!'
            | '?'
            | '\n'
            | '\r'
            | '，'
            | '；'
            | '：'
            | '。'
            | '！'
            | '？'
            | '、'
    )
}
fn align8(value: usize) -> usize {
    (value + 7) & !7
}
fn read_u32(bytes: &[u8], at: usize) -> Result<u32, FormatError> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or(FormatError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}
fn read_u16(bytes: &[u8], at: usize) -> Result<u16, FormatError> {
    Ok(u16::from_le_bytes(
        bytes
            .get(at..at + 2)
            .ok_or(FormatError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}
fn read_u64(bytes: &[u8], at: usize) -> Result<u64, FormatError> {
    Ok(u64::from_le_bytes(
        bytes
            .get(at..at + 8)
            .ok_or(FormatError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}
fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put_u64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// Structural format failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormatError {
    /// Input ended inside a required field.
    Truncated,
    /// Magic bytes do not identify this format.
    BadMagic,
    /// A component version is unsupported.
    UnsupportedVersion(&'static str, u32),
    /// An integer could not be represented safely.
    IntegerOverflow,
    /// Reserved bytes are nonzero.
    ReservedNonzero,
    /// A section kind is unknown.
    UnknownSection(u32),
    /// A section codec is unknown.
    UnknownCodec(u32),
    /// Required sections are absent.
    MissingSection,
    /// Sections are duplicated or not sorted.
    UnsortedOrDuplicateSections,
    /// A section offset, length, or alignment is invalid.
    InvalidOffset,
    /// Section byte ranges overlap.
    OverlappingSections,
    /// A finite-state section is malformed.
    MalformedFst,
    /// A variable integer is malformed or noncanonical.
    MalformedLeb128,
    /// A section's internal version-1 encoding is malformed.
    MalformedSection(SectionKind),
    /// A blocked offset table is malformed.
    MalformedOffsetTable,
    /// A requested dense identifier is outside its section.
    IdentifierOutOfRange,
    /// Text bytes are not valid UTF-8.
    InvalidUtf8,
    /// Stored text evidence contains unsupported derivation bits.
    UnknownDerivationFlags(u16),
    /// A morphology family contains an unsupported part-of-speech code.
    UnknownPartOfSpeech,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for FormatError {}

#[cfg(test)]
mod tests {
    use super::*;
    use fst::Map;
    use std::borrow::Cow;

    #[test]
    fn normalization_retains_digits_and_builds_spelling_aliases() {
        let normalized = normalize_text("3-D, don't");
        assert_eq!(normalized, "3-d don't");
        assert_eq!(normalize_text("Ｅ\u{301}COLE"), "école");
        assert_eq!(normalize_text("\u{301}accent"), "accent");
        assert!(spelling_views("3-d").contains(&("3d".to_owned(), DERIVATION_HYPHENS_JOINED)));
        assert!(spelling_views("3-d").contains(&("3 d".to_owned(), DERIVATION_HYPHENS_SEPARATED)));
        assert!(
            spelling_views("don't").contains(&("dont".to_owned(), DERIVATION_APOSTROPHE_REMOVED))
        );
    }

    #[test]
    fn normalization_borrows_only_already_normalized_input() {
        assert!(matches!(
            normalize_text_cow("water-melon"),
            Cow::Borrowed(_)
        ));
        assert!(matches!(normalize_text_cow("don't"), Cow::Borrowed(_)));
        assert!(matches!(normalize_text_cow("Watermelon"), Cow::Owned(_)));
        assert!(matches!(normalize_text_cow("-watermelon"), Cow::Owned(_)));
        assert!(matches!(
            normalize_text_cow("watermelon  fruit"),
            Cow::Owned(_)
        ));
    }

    #[test]
    fn pretokenized_query_views_match_the_public_helper() {
        let literal = tokenize_query("3-D don't");
        assert_eq!(query_views("3-D don't"), query_views_from_literal(&literal));
    }

    #[test]
    fn grammar_is_narrow_and_never_discards_standalone_words() {
        assert_eq!(
            reduce_optional_grammar(&["to", "be", "a", "doctor"]),
            Some(vec!["doctor".to_owned()])
        );
        assert_eq!(
            reduce_optional_grammar(&["a", "picture", "of", "the", "store"]),
            Some(
                vec!["picture", "of", "store"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            )
        );
        for word in ["is", "too", "a", "the"] {
            assert_eq!(reduce_optional_grammar(&[word]), None);
        }
        assert_eq!(reduce_optional_grammar(&["give", "up"]), None);
        assert_eq!(reduce_optional_grammar(&["my", "name", "is"]), None);
        for phrase in [
            &["to", "happy"][..],
            &["be", "happy"],
            &["to", "be", "happy"],
            &["a", "happy"],
            &["the", "happy"],
        ] {
            assert_eq!(
                reduce_optional_grammar(phrase),
                Some(vec!["happy".to_owned()])
            );
        }
    }

    #[test]
    fn query_ranges_and_hard_boundaries_refer_to_raw_utf8() {
        let segments = tokenize_query("École—3-D；don't");
        assert_eq!(segments.len(), 2);
        assert_eq!(
            segments[0]
                .tokens
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            ["école", "3-d"]
        );
        assert_eq!(
            &"École—3-D；don't"[segments[0].tokens[0].raw_range.clone()],
            "École"
        );
        assert_eq!(segments[1].tokens[0].text, "don't");
        let separated = query_views("3-D")
            .into_iter()
            .find(|view| view.derivation_flags == DERIVATION_HYPHENS_SEPARATED)
            .unwrap();
        assert_eq!(
            separated.segments[0]
                .tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<Vec<_>>(),
            ["3", "d"]
        );
        assert_eq!(
            separated.segments[0].tokens[0].raw_range,
            separated.segments[0].tokens[1].raw_range
        );
        assert_eq!(
            separated.segments[0].tokens[0].raw_group,
            separated.segments[0].tokens[1].raw_group
        );
    }

    #[test]
    fn typed_reader_exposes_search_and_morphology_records() {
        let bytes = typed_reader_fixture();
        let index = EnglishSearchIndex::parse(&bytes).unwrap();

        let medicine = index.token_id("medicine").unwrap().unwrap();
        let run = index.token_id("run").unwrap().unwrap();
        let shop = index.token_id("shop").unwrap().unwrap();
        assert_eq!(index.token_string(medicine).unwrap(), "medicine");
        assert_eq!(index.token_string(run).unwrap(), "run");
        assert_eq!(index.token_string(shop).unwrap(), "shop");
        assert_eq!(index.token_id("missing").unwrap(), None);

        let text_key = index.phrase_key("medicine shop").unwrap().unwrap();
        assert_eq!(text_key, TextKey::new(0));
        let text = index.text(text_key).unwrap();
        assert_eq!(text.token_count, 2);
        assert_eq!(
            text.tokens().collect::<Result<Vec<_>, _>>().unwrap(),
            [medicine, shop]
        );
        assert_eq!(
            text.bindings().collect::<Result<Vec<_>, _>>().unwrap(),
            [
                TextBinding {
                    runtime_key: RuntimeKey::new(2),
                    derivation_flags: 0,
                },
                TextBinding {
                    runtime_key: RuntimeKey::new(3),
                    derivation_flags: DERIVATION_GRAMMAR_REDUCED,
                },
            ]
        );

        let token = index.token(medicine).unwrap();
        assert_eq!(token.occurrence_count, 1);
        assert_eq!(token.document_count, 1);
        assert_eq!(token.hit_count, 2);
        assert_eq!(
            token
                .occurrences()
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            [TokenOccurrence {
                text_key,
                position: 0,
            }]
        );
        let hits = token.direct_hits().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.runtime_key).collect::<Vec<_>>(),
            [RuntimeKey::new(2), RuntimeKey::new(3)]
        );
        assert_eq!(
            hits[0].rank,
            DirectHitRank {
                containment: true,
                derived: false,
                parenthetical_omission: false,
                text_token_count: 2,
                transformation_count: 0,
            }
        );

        let analyses = index
            .morphology_analyses("running")
            .unwrap()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(analyses, [MorphFamilyId::new(0)]);
        let family = index.morphology_family(analyses[0]).unwrap();
        assert_eq!(family.part_of_speech, MorphologyPartOfSpeech::Verb);
        assert_eq!(family.lemma, "run");
        assert_eq!(
            family
                .corpus_tokens()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            [run]
        );
        let families = index
            .morphology_families()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            families
                .iter()
                .map(|family| (family.id, family.part_of_speech, family.lemma))
                .collect::<Vec<_>>(),
            [
                (MorphFamilyId::new(0), MorphologyPartOfSpeech::Verb, "run",),
                (MorphFamilyId::new(1), MorphologyPartOfSpeech::Noun, "shop",),
            ]
        );
        assert_eq!(
            families[1]
                .corpus_tokens()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            [shop]
        );
        assert!(index.morphology_analyses("runner").unwrap().is_none());

        let metadata = index.metadata().unwrap();
        assert_eq!(metadata.token_count, 3);
        assert_eq!(metadata.text_count, 1);
        assert_eq!(metadata.maximum_text_tokens, 2);
        assert_eq!(metadata.wordnet_revision, "3.1");
        assert_eq!(metadata.override_revision, "1");
        assert_eq!(
            metadata
                .section_sizes()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            [MetadataSectionSize {
                kind: SectionKind::TokenFst,
                bytes: 123,
            }]
        );
    }

    #[test]
    fn typed_reader_rejects_out_of_range_identifiers() {
        let bytes = typed_reader_fixture();
        let index = EnglishSearchIndex::parse(&bytes).unwrap();
        assert_eq!(
            index.text(TextKey::new(1)).unwrap_err(),
            FormatError::IdentifierOutOfRange
        );
        assert_eq!(
            index.token(TokenId::new(3)).unwrap_err(),
            FormatError::IdentifierOutOfRange
        );
        assert_eq!(
            index.morphology_family(MorphFamilyId::new(2)).unwrap_err(),
            FormatError::IdentifierOutOfRange
        );
    }

    #[test]
    fn container_rejects_damage() {
        let empty_map = Map::from_iter(std::iter::empty::<(&str, u64)>()).unwrap();
        let empty_fst = empty_map.as_fst().as_bytes().to_vec();
        let sections = (1..=13)
            .map(|raw| Section {
                kind: SectionKind::try_from(raw).unwrap(),
                codec: SectionCodec::RawV1,
                item_count: 0,
                bytes: if matches!(raw, 1 | 4 | 10) {
                    empty_fst.clone()
                } else {
                    Vec::new()
                },
            })
            .collect();
        let bytes = write_container(0, sections).unwrap();
        assert!(EnglishSearchIndex::parse(&bytes).is_ok());
        let mut bad = bytes.clone();
        bad[0] = 0;
        assert_eq!(
            EnglishSearchIndex::parse(&bad).unwrap_err(),
            FormatError::BadMagic
        );
        let mut bad = bytes;
        bad[8] = 2;
        assert_eq!(
            EnglishSearchIndex::parse(&bad).unwrap_err(),
            FormatError::UnsupportedVersion("search", 2)
        );

        let mut bad = write_test_container(&empty_fst);
        bad[28..32].copy_from_slice(&12_u32.to_le_bytes());
        assert_eq!(
            EnglishSearchIndex::parse(&bad).unwrap_err(),
            FormatError::MissingSection
        );

        let mut bad = write_test_container(&empty_fst);
        bad[HEADER_LEN + DIRECTORY_ENTRY_LEN..HEADER_LEN + DIRECTORY_ENTRY_LEN + 4]
            .copy_from_slice(&1_u32.to_le_bytes());
        assert_eq!(
            EnglishSearchIndex::parse(&bad).unwrap_err(),
            FormatError::UnsortedOrDuplicateSections
        );

        let mut bad = write_test_container(&empty_fst);
        let first_offset = read_u64(&bad, HEADER_LEN + 8).unwrap();
        put_u64(&mut bad, HEADER_LEN + 8, first_offset + 1);
        assert_eq!(
            EnglishSearchIndex::parse(&bad).unwrap_err(),
            FormatError::InvalidOffset
        );

        let mut bad = write_test_container(&empty_fst);
        let first_offset = read_u64(&bad, HEADER_LEN + 8).unwrap() as usize;
        bad[first_offset] ^= 0xff;
        assert_eq!(
            EnglishSearchIndex::parse(&bad).unwrap_err(),
            FormatError::MalformedFst
        );
    }

    fn write_test_container(empty_fst: &[u8]) -> Vec<u8> {
        write_container(
            0,
            (1..=13)
                .map(|raw| Section {
                    kind: SectionKind::try_from(raw).unwrap(),
                    codec: SectionCodec::RawV1,
                    item_count: 0,
                    bytes: if matches!(raw, 1 | 4 | 10) {
                        empty_fst.to_vec()
                    } else {
                        Vec::new()
                    },
                })
                .collect(),
        )
        .unwrap()
    }

    fn typed_reader_fixture() -> Vec<u8> {
        let tokens = ["medicine", "run", "shop"];
        let token_fst = Map::from_iter(
            tokens
                .iter()
                .enumerate()
                .map(|(id, token)| (*token, u64::try_from(id).unwrap())),
        )
        .unwrap()
        .as_fst()
        .as_bytes()
        .to_vec();
        let phrase_fst = Map::from_iter([("medicine shop", 0)]).unwrap();
        let morph_fst = Map::from_iter([("running", 0)]).unwrap();

        let token_strings = encode_test_strings(&tokens);
        let text_tokens = vec![0, 2];
        let text_bindings = vec![2, 0, 1, DERIVATION_GRAMMAR_REDUCED as u8];
        let token_offsets = encode_test_offsets(&[0, 2]);
        let binding_offsets = encode_test_offsets(&[0, 4]);
        let mut text_meta = Vec::new();
        text_meta.extend_from_slice(&1_u32.to_le_bytes());
        text_meta.extend_from_slice(&0_u32.to_le_bytes());
        text_meta.extend_from_slice(&(token_offsets.len() as u64).to_le_bytes());
        text_meta.extend_from_slice(&(binding_offsets.len() as u64).to_le_bytes());
        text_meta.extend_from_slice(&token_offsets);
        text_meta.extend_from_slice(&binding_offsets);
        text_meta.extend_from_slice(&2_u32.to_le_bytes());
        text_meta.extend_from_slice(&0_u16.to_le_bytes());
        text_meta.extend_from_slice(&0_u16.to_le_bytes());

        let occurrence_lists = [
            encode_test_occurrences(&[(0, 0)]),
            encode_test_occurrences(&[]),
            encode_test_occurrences(&[(0, 1)]),
        ];
        let mut occurrence_bytes = Vec::new();
        let mut occurrence_offsets = vec![0_u64];
        for list in occurrence_lists {
            occurrence_bytes.extend_from_slice(&list);
            occurrence_offsets.push(occurrence_bytes.len() as u64);
        }
        let hit_lists = [vec![1, 4, 2, 0, 2, 2, 1], vec![0], vec![0]];
        let mut hit_bytes = Vec::new();
        let mut hit_offsets = vec![0_u64];
        for list in hit_lists {
            hit_bytes.extend_from_slice(&list);
            hit_offsets.push(hit_bytes.len() as u64);
        }
        let occurrence_table = encode_test_offsets(&occurrence_offsets);
        let hit_table = encode_test_offsets(&hit_offsets);
        let mut token_meta = Vec::new();
        token_meta.extend_from_slice(&3_u32.to_le_bytes());
        token_meta.extend_from_slice(&0_u32.to_le_bytes());
        for count in [1_u32, 0, 1, 1, 0, 1] {
            token_meta.extend_from_slice(&count.to_le_bytes());
        }
        token_meta.extend_from_slice(&(occurrence_table.len() as u64).to_le_bytes());
        token_meta.extend_from_slice(&(hit_table.len() as u64).to_le_bytes());
        token_meta.extend_from_slice(&occurrence_table);
        token_meta.extend_from_slice(&hit_table);
        for count in [2_u32, 0, 0] {
            token_meta.extend_from_slice(&count.to_le_bytes());
        }

        let morph_analyses = vec![1, 0];
        let morph_tokens = vec![
            2, 3, b'r', b'u', b'n', 1, 1, 1, 4, b's', b'h', b'o', b'p', 1, 2,
        ];
        let mut metadata = Vec::new();
        for value in [
            SEARCH_FORMAT_VERSION,
            NORMALIZATION_VERSION,
            GRAMMAR_VERSION,
            MORPHOLOGY_VERSION,
            3,
            1,
            2,
            2,
            2,
            2,
            1,
            2,
            13,
        ] {
            metadata.extend_from_slice(&value.to_le_bytes());
        }
        for revision in ["3.1", "1"] {
            metadata.extend_from_slice(&(revision.len() as u32).to_le_bytes());
            metadata.extend_from_slice(revision.as_bytes());
        }
        metadata.extend_from_slice(&1_u32.to_le_bytes());
        metadata.extend_from_slice(&(SectionKind::TokenFst as u32).to_le_bytes());
        metadata.extend_from_slice(&123_u64.to_le_bytes());

        let section = |kind, item_count, bytes| Section {
            kind,
            codec: SectionCodec::RawV1,
            item_count,
            bytes,
        };
        write_container(
            4,
            vec![
                section(SectionKind::TokenFst, 3, token_fst),
                section(SectionKind::TokenStrings, 3, token_strings),
                section(SectionKind::TokenMeta, 3, token_meta),
                section(
                    SectionKind::PhraseFst,
                    1,
                    phrase_fst.as_fst().as_bytes().to_vec(),
                ),
                section(SectionKind::TextMeta, 1, text_meta),
                section(SectionKind::TextTokens, 1, text_tokens),
                section(SectionKind::TextBindings, 2, text_bindings),
                section(SectionKind::TokenOccurrences, 3, occurrence_bytes),
                section(SectionKind::TokenHits, 3, hit_bytes),
                section(
                    SectionKind::MorphologyFst,
                    1,
                    morph_fst.as_fst().as_bytes().to_vec(),
                ),
                section(SectionKind::MorphologyAnalyses, 1, morph_analyses),
                section(SectionKind::MorphologyTokens, 2, morph_tokens),
                section(SectionKind::Metadata, 1, metadata),
            ],
        )
        .unwrap()
    }

    fn encode_test_strings(strings: &[&str]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut offsets = vec![0_u64];
        for string in strings {
            data.extend_from_slice(string.as_bytes());
            offsets.push(data.len() as u64);
        }
        let table = encode_test_offsets(&offsets);
        let mut result = Vec::new();
        result.extend_from_slice(&(strings.len() as u32).to_le_bytes());
        result.extend_from_slice(&0_u32.to_le_bytes());
        result.extend_from_slice(&(table.len() as u64).to_le_bytes());
        result.extend_from_slice(&table);
        result.extend_from_slice(&data);
        result
    }

    fn encode_test_offsets(offsets: &[u64]) -> Vec<u8> {
        let blocks = offsets.chunks(128).collect::<Vec<_>>();
        let mut result = Vec::new();
        result.extend_from_slice(&(offsets.len() as u32).to_le_bytes());
        result.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
        let directory_at = result.len();
        result.resize(directory_at + blocks.len() * 24, 0);
        let mut payload = Vec::new();
        for (block_index, block) in blocks.into_iter().enumerate() {
            let at = directory_at + block_index * 24;
            result[at..at + 4].copy_from_slice(&((block_index * 128) as u32).to_le_bytes());
            result[at + 4..at + 8].copy_from_slice(&(block.len() as u32).to_le_bytes());
            result[at + 8..at + 16].copy_from_slice(&block[0].to_le_bytes());
            result[at + 16..at + 24].copy_from_slice(&(payload.len() as u64).to_le_bytes());
            for pair in block.windows(2) {
                write_uleb128(pair[1] - pair[0], &mut payload);
            }
        }
        result.extend_from_slice(&payload);
        result
    }

    fn encode_test_occurrences(records: &[(u32, u32)]) -> Vec<u8> {
        if records.is_empty() {
            return 0_u32.to_le_bytes().to_vec();
        }
        let first = records[0].0;
        let mut payload = Vec::new();
        let mut previous = first;
        for &(text, position) in records {
            write_uleb128(u64::from(text - previous), &mut payload);
            write_uleb128(u64::from(position), &mut payload);
            previous = text;
        }
        let mut result = Vec::new();
        result.extend_from_slice(&1_u32.to_le_bytes());
        result.extend_from_slice(&first.to_le_bytes());
        result.extend_from_slice(&(records.len() as u32).to_le_bytes());
        result.extend_from_slice(&0_u64.to_le_bytes());
        write_uleb128(payload.len() as u64, &mut result);
        result.extend_from_slice(&payload);
        result
    }
}
