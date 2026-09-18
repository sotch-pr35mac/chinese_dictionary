//! Consumer-local schema-4 model and stable lexical identity primitives.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;
use unicode_normalization::UnicodeNormalization;

/// Incompatible schema version wrapped around every binary artifact.
#[allow(dead_code)]
pub const SCHEMA_VERSION: u32 = 4;
/// Version prefix used by persistent lexical identifiers.
pub const IDENTITY_VERSION: u8 = 1;

/// Persistent identity derived from normalized headwords and canonical Pinyin.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub struct LexicalId(pub(crate) [u8; 32]);

impl Serialize for LexicalId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut buffer = [0_u8; 66];
        serializer.serialize_str(self.format_into(&mut buffer))
    }
}

impl<'de> Deserialize<'de> for LexicalId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Cow::<str>::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

impl LexicalId {
    /// Computes an identity from simplified, traditional, and numbered-Pinyin fields.
    pub fn new(
        simplified: &str,
        traditional: &str,
        numbered_pinyin: &str,
    ) -> Result<Self, ModelError> {
        Ok(Self::from_preimage(&Self::prehash_bytes(
            simplified,
            traditional,
            numbered_pinyin,
        )?))
    }

    /// Hashes an already canonical identity preimage and adds the version prefix.
    pub fn from_preimage(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Validates and wraps a serialized lexical identifier.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ModelError> {
        let value = value.as_ref();
        let digest = value
            .strip_prefix("1:")
            .ok_or(ModelError::InvalidLexicalId)?;
        if digest.len() != 64 {
            return Err(ModelError::InvalidLexicalId);
        }
        let mut decoded = [0_u8; 32];
        for (index, pair) in digest.as_bytes().chunks_exact(2).enumerate() {
            decoded[index] = (decode_hex(pair[0]).ok_or(ModelError::InvalidLexicalId)? << 4)
                | decode_hex(pair[1]).ok_or(ModelError::InvalidLexicalId)?;
        }
        Ok(Self(decoded))
    }

    /// Returns the identity format version.
    pub const fn version(&self) -> u8 {
        IDENTITY_VERSION
    }

    /// Returns the fixed-size SHA-256 identity digest.
    pub const fn digest(&self) -> &[u8; 32] {
        &self.0
    }

    /// Formats the external identity representation into caller-owned storage.
    pub fn format_into<'a>(&self, buffer: &'a mut [u8; 66]) -> &'a str {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        buffer[0] = b'1';
        buffer[1] = b':';
        for (index, byte) in self.0.iter().copied().enumerate() {
            buffer[2 + index * 2] = HEX[usize::from(byte >> 4)];
            buffer[3 + index * 2] = HEX[usize::from(byte & 0x0f)];
        }
        std::str::from_utf8(buffer).expect("identity formatting only writes ASCII")
    }

    /// Produces the exact NUL-separated bytes used as the SHA-256 preimage.
    pub fn prehash_bytes(
        simplified: &str,
        traditional: &str,
        numbered_pinyin: &str,
    ) -> Result<Vec<u8>, ModelError> {
        let simplified = normalize_headword(simplified)?;
        let traditional = normalize_headword(traditional)?;
        let numbered_pinyin = canonicalize_numbered_pinyin(numbered_pinyin)?;
        let mut bytes =
            Vec::with_capacity(simplified.len() + traditional.len() + numbered_pinyin.len() + 2);
        bytes.extend_from_slice(simplified.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(traditional.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(numbered_pinyin.as_bytes());
        Ok(bytes)
    }

    /// Constructs an identity from a validated digest stored in the archive.
    pub(crate) const fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest)
    }
}

impl fmt::Display for LexicalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buffer = [0_u8; 66];
        formatter.write_str(self.format_into(&mut buffer))
    }
}

impl FromStr for LexicalId {
    type Err = ModelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

fn decode_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Normalizes a headword with NFC and outer trimming and rejects unsafe values.
pub fn normalize_headword(value: &str) -> Result<Cow<'_, str>, ModelError> {
    let trimmed = value.trim();
    let normalized = if unicode_normalization::is_nfc(trimmed) {
        Cow::Borrowed(trimmed)
    } else {
        Cow::Owned(trimmed.nfc().collect::<String>())
    };
    if normalized.is_empty() {
        return Err(ModelError::EmptyHeadword);
    }
    if normalized.chars().any(char::is_control) {
        return Err(ModelError::InvalidHeadword);
    }
    Ok(normalized)
}

/// Canonicalizes and validates numbered Hanyu Pinyin for stable identity input.
pub fn canonicalize_numbered_pinyin(value: &str) -> Result<String, ModelError> {
    let normalized = value.nfc().collect::<String>();
    if normalized.trim().is_empty() {
        return Err(ModelError::InvalidPinyin);
    }

    let mut canonical = String::with_capacity(normalized.len());
    let mut letters = String::new();
    for character in normalized.chars() {
        if character.is_ascii_alphabetic() || matches!(character, 'ü' | 'Ü') {
            letters.push(character);
        } else if character == ':' {
            let previous = letters.pop().ok_or(ModelError::InvalidPinyin)?;
            letters.push(match previous {
                'u' => 'ü',
                'U' => 'Ü',
                _ => return Err(ModelError::InvalidPinyin),
            });
        } else if let Some(tone) = character.to_digit(10) {
            if !(1..=5).contains(&tone) || letters.is_empty() {
                return Err(ModelError::InvalidPinyin);
            }
            let syllable = letters
                .chars()
                .map(|letter| match letter {
                    'v' => 'ü',
                    'V' => 'Ü',
                    other => other,
                })
                .collect::<String>();
            if !is_valid_pinyin_syllable(&syllable) {
                return Err(ModelError::InvalidPinyin);
            }
            canonical.push_str(&syllable);
            canonical.push(char::from_digit(tone, 10).expect("validated decimal tone"));
            letters.clear();
        } else if character.is_whitespace() || matches!(character, '-' | '\'' | '’' | ',' | '·')
        {
            if !letters.is_empty() {
                return Err(ModelError::InvalidPinyin);
            }
        } else {
            return Err(ModelError::InvalidPinyin);
        }
    }
    if !letters.is_empty() || canonical.is_empty() || canonical == "xx5" {
        return Err(ModelError::InvalidPinyin);
    }
    Ok(canonical)
}

fn is_valid_pinyin_syllable(value: &str) -> bool {
    const VALID: &str = "a ai an ang ao ba bai ban bang bao bei ben beng bi bian biao bie bin bing bo bu ca cai can cang cao ce cen ceng cha chai chan chang chao che chen cheng chi chong chou chu chua chuai chuan chuang chui chun chuo ci cong cou cu cuan cui cun cuo da dai dan dang dao de dei den deng di dia dian diao die ding diu dong dou du duan dui dun duo e ei en eng er fa fan fang fei fen feng fo fou fu ga gai gan gang gao ge gei gen geng gong gou gu gua guai guan guang gui gun guo ha hai han hang hao he hei hen heng hm hng hong hou hu hua huai huan huang hui hun huo ji jia jian jiang jiao jie jin jing jiong jiu ju juan jue jun ka kai kan kang kao ke kei ken keng kong kou ku kua kuai kuan kuang kui kun kuo la lai lan lang lao le lei leng li lia lian liang liao lie lin ling liu lo long lou lu lü luan lüan lue lüe lun luo m ma mai man mang mao me mei men meng mi mian miao mie min ming miu mo mou mu n na nai nan nang nao ne nei nen neng ng ni nian niang niao nie nin ning niu nong nou nu nü nuan nüan nue nüe nun nuo o ou pa pai pan pang pao pei pen peng pi pian piao pie pin ping po pou pu qi qia qian qiang qiao qie qin qing qiong qiu qu quan que qun r ran rang rao re ren reng ri rong rou ru rua ruan rui run ruo sa sai san sang sao se sen seng sha shai shan shang shao she shei shen sheng shi shou shu shua shuai shuan shuang shui shun shuo si song sou su suan sui sun suo ta tai tan tang tao te teng ti tian tiao tie ting tong tou tu tuan tui tun tuo wa wai wan wang wei wen weng wo wu xi xia xian xiang xiao xie xin xing xiong xiu xu xuan xue xun ya yan yang yao ye yi yin ying yong you yu yuan yue yun za zai zan zang zao ze zei zen zeng zha zhai zhan zhang zhao zhe zhei zhen zheng zhi zhong zhou zhu zhua zhuai zhuan zhuang zhui zhun zhuo zi zong zou zu zuan zui zun zuo";
    let lowercase = value.to_lowercase();
    VALID.split_ascii_whitespace().any(|item| item == lowercase)
}

/// Failure modes for lexical identity and text normalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelError {
    /// A headword contains no characters after outer trimming.
    EmptyHeadword,
    /// A headword contains a control character.
    InvalidHeadword,
    /// A serialized lexical identifier has the wrong version or digest form.
    InvalidLexicalId,
    /// Numbered Pinyin is not a complete sequence of reviewed syllables and tones.
    InvalidPinyin,
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ModelError {}

/// Canonical Pinyin representations stored together for display and lookup.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
pub struct Pinyin {
    /// Tone-marked, space-separated display form.
    pub marks: String,
    /// Concatenated syllable-plus-tone identity and lookup form.
    pub numbers: String,
    /// Tone number for each syllable, in order.
    pub tones: Vec<u8>,
}

/// Upstream dictionary that supports a published value.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// CC-CEDICT.
    CcCedict,
    /// Chinese Notes.
    ChineseNotes,
    /// English Wiktionary.
    Wiktionary,
}

/// A value paired with one or more ordered source attributions.
#[derive(
    Archive, RkyvDeserialize, RkyvSerialize, Clone, Debug, Eq, PartialEq, Serialize, Deserialize,
)]
pub struct Sourced<T> {
    /// Published value.
    pub value: T,
    /// Ordered, deduplicated sources supporting the value.
    pub sources: Vec<Source>,
}

impl<T> Sourced<T> {
    /// Creates a value attributed to one source.
    pub fn one(value: T, source: Source) -> Self {
        Self {
            value,
            sources: vec![source],
        }
    }
}

/// A pronunciation variant that does not exist as its own lexical entity.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
pub struct AlternativePronunciation {
    /// Canonical alternate Pinyin.
    pub pronunciation: Pinyin,
    /// Source-provided scope label such as `Taiwan pr.` or `also pr.`.
    pub label: String,
}

/// Structured bilingual usage example.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
pub struct Example {
    /// Simplified Chinese example text when known.
    pub simplified: Option<String>,
    /// Traditional Chinese example text when known.
    pub traditional: Option<String>,
    /// English translation when the source provides one.
    pub english: Option<String>,
}

/// Reviewed semantic category for a structured qualifier.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum QualifierCategory {
    /// Subject area or professional domain.
    Domain,
    /// Formality or social register.
    Register,
    /// Geographic variety.
    Region,
    /// Usage condition.
    Usage,
    /// Grammatical or lexical restriction.
    Restriction,
    /// Informational note.
    Information,
    /// Explanatory annotation.
    Explanation,
}

/// Reviewed structured qualifier attached to a definition.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
pub struct Qualifier {
    /// Semantic category of the qualifier.
    pub category: QualifierCategory,
    /// Normalized source value.
    pub value: String,
}

/// Reviewed kinds of lexical entity that are not ordinary parts of speech.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum LexicalKind {
    /// Source boilerplate entry.
    Boilerplate,
    /// Form that cannot occur independently.
    BoundForm,
    /// Classifier or measure-word entry.
    Classifier,
    /// Contracted form.
    Contraction,
    /// Conventional expression.
    Expression,
    /// Foreign or borrowed form.
    Foreign,
    /// Infix.
    Infix,
    /// Idiomatic expression.
    Idiom,
    /// Productive lexical pattern.
    Pattern,
    /// Multiword phrase.
    Phrase,
    /// Phonetic component or use.
    Phonetic,
    /// Prefix.
    Prefix,
    /// Proverb.
    Proverb,
    /// Character radical.
    Radical,
    /// Fixed or set phrase.
    SetPhrase,
    /// Suffix.
    Suffix,
}

/// Reviewed grammatical parts of speech.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum PartOfSpeech {
    /// Adjective.
    Adjective,
    /// Adverb.
    Adverb,
    /// Auxiliary verb.
    AuxiliaryVerb,
    /// Conjunction.
    Conjunction,
    /// Determiner.
    Determiner,
    /// Interjection.
    Interjection,
    /// Interrogative pronoun.
    InterrogativePronoun,
    /// Measure word.
    MeasureWord,
    /// Noun.
    Noun,
    /// Number.
    Number,
    /// Onomatopoeia.
    Onomatopoeia,
    /// Ordinal.
    Ordinal,
    /// Particle.
    Particle,
    /// Postposition.
    Postposition,
    /// Preposition.
    Preposition,
    /// Pronoun.
    Pronoun,
    /// Proper noun.
    ProperNoun,
    /// Quantity expression.
    Quantity,
    /// Verb.
    Verb,
}

/// Reviewed Chinese varieties attached to a classifier reference.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ChineseVariety {
    Mandarin,
    Sichuanese,
    Dungan,
    Cantonese,
    Taishanese,
    Gan,
    Hakka,
    Jin,
    NorthernMin,
    EasternMin,
    MiddleChinese,
    Hokkien,
    Teochew,
    LeizhouMin,
    PuxianMin,
    SouthernPinghua,
    Wu,
    Xiang,
    LoudiXiang,
    HengyangXiang,
    OldChinese,
}

/// Chinese forms and an optional exact lexical target for a classifier.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
pub struct MeasureWordReference {
    pub traditional: String,
    pub simplified: String,
    pub lexical_id: Option<LexicalId>,
    pub varieties: Vec<ChineseVariety>,
}

/// One ordered English definition and its independently attributed metadata.
#[derive(
    Archive, RkyvDeserialize, RkyvSerialize, Clone, Debug, Eq, PartialEq, Serialize, Deserialize,
)]
pub struct Definition {
    /// Leaf English gloss.
    pub gloss: Sourced<String>,
    /// Structured usage examples.
    pub examples: Vec<Sourced<Example>>,
    /// Substantive explanatory prose.
    pub commentary: Vec<Sourced<String>>,
    /// Reviewed structured qualifiers.
    pub qualifiers: Vec<Sourced<Qualifier>>,
    /// Reviewed lexical kinds.
    pub lexical_kinds: Vec<Sourced<LexicalKind>>,
    /// Reviewed parts of speech.
    pub parts_of_speech: Vec<Sourced<PartOfSpeech>>,
    /// Pronunciations scoped to this definition.
    pub alternative_pronunciations: Vec<Sourced<AlternativePronunciation>>,
    /// Classifier references scoped to this definition.
    pub measure_words: Vec<Sourced<MeasureWordReference>>,
}

impl Definition {
    /// Creates a definition with an attributed gloss and empty metadata vectors.
    pub fn new(gloss: String, source: Source) -> Self {
        Self {
            gloss: Sourced::one(gloss, source),
            examples: Vec::new(),
            commentary: Vec::new(),
            qualifiers: Vec::new(),
            lexical_kinds: Vec::new(),
            parts_of_speech: Vec::new(),
            alternative_pronunciations: Vec::new(),
            measure_words: Vec::new(),
        }
    }
}

/// Closed proficiency level shared by the supported HSK systems.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
)]
pub enum HskLevel {
    /// Level one.
    One,
    /// Level two.
    Two,
    /// Level three.
    Three,
    /// Level four.
    Four,
    /// Level five.
    Five,
    /// Level six.
    Six,
    /// Combined levels seven through nine.
    SevenToNine,
}

/// HSK memberships across historical and current standards.
#[derive(
    Archive,
    RkyvDeserialize,
    RkyvSerialize,
    Clone,
    Debug,
    Default,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
)]
pub struct HskLevels {
    /// Memberships in the 2015 HSK vocabulary.
    pub hsk_2015: Vec<HskLevel>,
    /// Memberships in the 2021 Chinese proficiency standard.
    pub proficiency_standard_2021: Vec<HskLevel>,
    /// Memberships in the 2025 HSK exam syllabus.
    pub hsk_exam_syllabus_2025: Vec<HskLevel>,
}

/// Canonical published dictionary entity for one exact identity tuple.
#[derive(
    Archive, RkyvDeserialize, RkyvSerialize, Clone, Debug, PartialEq, Serialize, Deserialize,
)]
pub struct LexicalUnit {
    /// Persistent content-derived identity.
    pub id: LexicalId,
    /// NFC-normalized simplified headword.
    pub simplified: String,
    /// NFC-normalized traditional headword.
    pub traditional: String,
    /// Primary Mandarin pronunciation.
    pub pinyin: Pinyin,
    /// Document-normalized commonness score; zero means unseen in the frequency corpora.
    pub commonness: f32,
    /// Entity-scoped pronunciation variants without their own lexical entity.
    pub alternative_pronunciations: Vec<Sourced<AlternativePronunciation>>,
    /// Entity-scoped classifier references.
    pub measure_words: Vec<Sourced<MeasureWordReference>>,
    /// HSK proficiency memberships.
    pub hsk: HskLevels,
    /// Ordered English definitions.
    pub english: Vec<Definition>,
}

/// Zero-copy view of one lexical unit stored in the embedded archive.
#[derive(Clone, Copy)]
pub struct LexicalUnitRef<'a> {
    inner: &'a ArchivedLexicalUnit,
}

impl<'a> LexicalUnitRef<'a> {
    pub(crate) const fn new(inner: &'a ArchivedLexicalUnit) -> Self {
        Self { inner }
    }

    /// Persistent content-derived identity.
    pub fn id(self) -> LexicalId {
        LexicalId::from_digest(self.inner.id.0)
    }

    /// NFC-normalized simplified headword.
    pub fn simplified(self) -> &'a str {
        self.inner.simplified.as_str()
    }

    /// NFC-normalized traditional headword.
    pub fn traditional(self) -> &'a str {
        self.inner.traditional.as_str()
    }

    /// Primary Mandarin pronunciation.
    pub fn pinyin(self) -> PinyinRef<'a> {
        PinyinRef {
            inner: &self.inner.pinyin,
        }
    }

    /// Document-normalized commonness score used as a ranking prior.
    pub fn commonness(self) -> f32 {
        self.inner.commonness.to_native()
    }

    /// Entity-scoped pronunciation variants.
    pub fn alternative_pronunciations(
        self,
    ) -> impl ExactSizeIterator<Item = SourcedAlternativePronunciationRef<'a>> + 'a {
        self.inner
            .alternative_pronunciations
            .iter()
            .map(|inner| SourcedAlternativePronunciationRef { inner })
    }

    /// Entity-scoped classifier references.
    pub fn measure_words(
        self,
    ) -> impl ExactSizeIterator<Item = SourcedMeasureWordReferenceRef<'a>> + 'a {
        self.inner
            .measure_words
            .iter()
            .map(|inner| SourcedMeasureWordReferenceRef { inner })
    }

    /// HSK proficiency memberships.
    pub fn hsk(self) -> HskLevelsRef<'a> {
        HskLevelsRef {
            inner: &self.inner.hsk,
        }
    }

    /// Ordered English definitions.
    pub fn english(self) -> impl ExactSizeIterator<Item = DefinitionRef<'a>> + 'a {
        self.inner
            .english
            .iter()
            .map(|inner| DefinitionRef { inner })
    }

    /// Materializes this archived view as the owned lexical model.
    pub fn to_owned(self) -> LexicalUnit {
        rkyv::deserialize::<LexicalUnit, rkyv::rancor::Error>(self.inner)
            .expect("build.rs validated the lexical archive")
    }
}

impl fmt::Debug for LexicalUnitRef<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LexicalUnitRef")
            .field("id", &self.id())
            .field("simplified", &self.simplified())
            .field("traditional", &self.traditional())
            .finish_non_exhaustive()
    }
}

impl PartialEq for LexicalUnitRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for LexicalUnitRef<'_> {}

/// Zero-copy classifier display forms, varieties, and optional lexical target.
#[derive(Clone, Copy)]
pub struct MeasureWordReferenceRef<'a> {
    inner: &'a ArchivedMeasureWordReference,
}

impl<'a> MeasureWordReferenceRef<'a> {
    /// Traditional Chinese classifier form.
    pub fn traditional(self) -> &'a str {
        self.inner.traditional.as_str()
    }

    /// Simplified Chinese classifier form.
    pub fn simplified(self) -> &'a str {
        self.inner.simplified.as_str()
    }

    /// Exact lexical target when one was uniquely resolved during bundle creation.
    pub fn lexical_id(self) -> Option<LexicalId> {
        self.inner
            .lexical_id
            .as_ref()
            .map(|value| LexicalId::from_digest(value.0))
    }

    /// Reviewed varieties in which this classifier applies.
    pub fn varieties(self) -> impl ExactSizeIterator<Item = ChineseVariety> + 'a {
        self.inner.varieties.iter().map(chinese_variety)
    }

    /// Materializes this archived view.
    pub fn to_owned(self) -> MeasureWordReference {
        rkyv::deserialize::<MeasureWordReference, rkyv::rancor::Error>(self.inner)
            .expect("build.rs validated the lexical archive")
    }
}

/// Borrowed primary or alternate Pinyin.
#[derive(Clone, Copy)]
pub struct PinyinRef<'a> {
    inner: &'a ArchivedPinyin,
}

impl<'a> PinyinRef<'a> {
    /// Tone-marked, space-separated display form.
    pub fn marks(self) -> &'a str {
        self.inner.marks.as_str()
    }

    /// Concatenated numbered identity and lookup form.
    pub fn numbers(self) -> &'a str {
        self.inner.numbers.as_str()
    }

    /// Tone number for each syllable.
    pub fn tones(self) -> &'a [u8] {
        self.inner.tones.as_slice()
    }

    /// Materializes this view.
    pub fn to_owned(self) -> Pinyin {
        rkyv::deserialize::<Pinyin, rkyv::rancor::Error>(self.inner)
            .expect("build.rs validated the lexical archive")
    }
}

/// Borrowed pronunciation variant.
#[derive(Clone, Copy)]
pub struct AlternativePronunciationRef<'a> {
    inner: &'a ArchivedAlternativePronunciation,
}

impl<'a> AlternativePronunciationRef<'a> {
    /// Canonical alternate Pinyin.
    pub fn pronunciation(self) -> PinyinRef<'a> {
        PinyinRef {
            inner: &self.inner.pronunciation,
        }
    }

    /// Source-provided scope label.
    pub fn label(self) -> &'a str {
        self.inner.label.as_str()
    }

    /// Materializes this view.
    pub fn to_owned(self) -> AlternativePronunciation {
        rkyv::deserialize::<AlternativePronunciation, rkyv::rancor::Error>(self.inner)
            .expect("build.rs validated the lexical archive")
    }
}

/// Borrowed bilingual usage example.
#[derive(Clone, Copy)]
pub struct ExampleRef<'a> {
    inner: &'a ArchivedExample,
}

impl<'a> ExampleRef<'a> {
    /// Simplified Chinese example text, when known.
    pub fn simplified(self) -> Option<&'a str> {
        self.inner.simplified.as_ref().map(|value| value.as_str())
    }

    /// Traditional Chinese example text, when known.
    pub fn traditional(self) -> Option<&'a str> {
        self.inner.traditional.as_ref().map(|value| value.as_str())
    }

    /// English translation, when present.
    pub fn english(self) -> Option<&'a str> {
        self.inner.english.as_ref().map(|value| value.as_str())
    }

    /// Materializes this view.
    pub fn to_owned(self) -> Example {
        rkyv::deserialize::<Example, rkyv::rancor::Error>(self.inner)
            .expect("build.rs validated the lexical archive")
    }
}

/// Borrowed structured qualifier.
#[derive(Clone, Copy)]
pub struct QualifierRef<'a> {
    inner: &'a ArchivedQualifier,
}

impl<'a> QualifierRef<'a> {
    /// Semantic qualifier category.
    pub fn category(self) -> QualifierCategory {
        qualifier_category(&self.inner.category)
    }

    /// Normalized source value.
    pub fn value(self) -> &'a str {
        self.inner.value.as_str()
    }

    /// Materializes this view.
    pub fn to_owned(self) -> Qualifier {
        rkyv::deserialize::<Qualifier, rkyv::rancor::Error>(self.inner)
            .expect("build.rs validated the lexical archive")
    }
}

/// Borrowed HSK memberships.
#[derive(Clone, Copy)]
pub struct HskLevelsRef<'a> {
    inner: &'a ArchivedHskLevels,
}

impl<'a> HskLevelsRef<'a> {
    /// Memberships in the 2015 HSK vocabulary.
    pub fn hsk_2015(self) -> impl ExactSizeIterator<Item = HskLevel> + 'a {
        self.inner.hsk_2015.iter().map(hsk_level)
    }

    /// Memberships in the 2021 Chinese proficiency standard.
    pub fn proficiency_standard_2021(self) -> impl ExactSizeIterator<Item = HskLevel> + 'a {
        self.inner.proficiency_standard_2021.iter().map(hsk_level)
    }

    /// Memberships in the 2025 HSK exam syllabus.
    pub fn hsk_exam_syllabus_2025(self) -> impl ExactSizeIterator<Item = HskLevel> + 'a {
        self.inner.hsk_exam_syllabus_2025.iter().map(hsk_level)
    }

    /// Materializes this view.
    pub fn to_owned(self) -> HskLevels {
        rkyv::deserialize::<HskLevels, rkyv::rancor::Error>(self.inner)
            .expect("build.rs validated the lexical archive")
    }
}

macro_rules! sourced_ref {
    ($name:ident, $archived:ty, $value:ty, $owned:ty, $value_expr:expr) => {
        #[doc = concat!("Borrowed sourced `", stringify!($value), "` value.")]
        #[derive(Clone, Copy)]
        pub struct $name<'a> {
            inner: &'a ArchivedSourced<$archived>,
        }

        impl<'a> $name<'a> {
            /// Published value.
            pub fn value(self) -> $value {
                ($value_expr)(self.inner)
            }

            /// Ordered source attributions.
            pub fn sources(self) -> impl ExactSizeIterator<Item = Source> + 'a {
                self.inner.sources.iter().map(source)
            }

            /// Materializes this sourced value.
            pub fn to_owned(self) -> Sourced<$owned> {
                rkyv::deserialize::<Sourced<$owned>, rkyv::rancor::Error>(self.inner)
                    .expect("build.rs validated the lexical archive")
            }
        }
    };
}

sourced_ref!(
    SourcedStringRef,
    String,
    &'a str,
    String,
    |value: &'a ArchivedSourced<String>| value.value.as_str()
);
sourced_ref!(
    SourcedMeasureWordReferenceRef,
    MeasureWordReference,
    MeasureWordReferenceRef<'a>,
    MeasureWordReference,
    |value: &'a ArchivedSourced<MeasureWordReference>| MeasureWordReferenceRef {
        inner: &value.value
    }
);
sourced_ref!(
    SourcedExampleRef,
    Example,
    ExampleRef<'a>,
    Example,
    |value: &'a ArchivedSourced<Example>| ExampleRef {
        inner: &value.value
    }
);
sourced_ref!(
    SourcedQualifierRef,
    Qualifier,
    QualifierRef<'a>,
    Qualifier,
    |value: &'a ArchivedSourced<Qualifier>| QualifierRef {
        inner: &value.value
    }
);
sourced_ref!(
    SourcedLexicalKindRef,
    LexicalKind,
    LexicalKind,
    LexicalKind,
    |value: &'a ArchivedSourced<LexicalKind>| lexical_kind(&value.value)
);
sourced_ref!(
    SourcedPartOfSpeechRef,
    PartOfSpeech,
    PartOfSpeech,
    PartOfSpeech,
    |value: &'a ArchivedSourced<PartOfSpeech>| part_of_speech(&value.value)
);
sourced_ref!(
    SourcedAlternativePronunciationRef,
    AlternativePronunciation,
    AlternativePronunciationRef<'a>,
    AlternativePronunciation,
    |value: &'a ArchivedSourced<AlternativePronunciation>| AlternativePronunciationRef {
        inner: &value.value
    }
);

/// Borrowed ordered English definition and metadata.
#[derive(Clone, Copy)]
pub struct DefinitionRef<'a> {
    inner: &'a ArchivedDefinition,
}

impl<'a> DefinitionRef<'a> {
    /// Leaf English gloss.
    pub fn gloss(self) -> SourcedStringRef<'a> {
        SourcedStringRef {
            inner: &self.inner.gloss,
        }
    }
    /// Structured usage examples.
    pub fn examples(self) -> impl ExactSizeIterator<Item = SourcedExampleRef<'a>> + 'a {
        self.inner
            .examples
            .iter()
            .map(|inner| SourcedExampleRef { inner })
    }
    /// Explanatory prose.
    pub fn commentary(self) -> impl ExactSizeIterator<Item = SourcedStringRef<'a>> + 'a {
        self.inner
            .commentary
            .iter()
            .map(|inner| SourcedStringRef { inner })
    }
    /// Reviewed qualifiers.
    pub fn qualifiers(self) -> impl ExactSizeIterator<Item = SourcedQualifierRef<'a>> + 'a {
        self.inner
            .qualifiers
            .iter()
            .map(|inner| SourcedQualifierRef { inner })
    }
    /// Reviewed lexical kinds.
    pub fn lexical_kinds(self) -> impl ExactSizeIterator<Item = SourcedLexicalKindRef<'a>> + 'a {
        self.inner
            .lexical_kinds
            .iter()
            .map(|inner| SourcedLexicalKindRef { inner })
    }
    /// Reviewed parts of speech.
    pub fn parts_of_speech(self) -> impl ExactSizeIterator<Item = SourcedPartOfSpeechRef<'a>> + 'a {
        self.inner
            .parts_of_speech
            .iter()
            .map(|inner| SourcedPartOfSpeechRef { inner })
    }
    /// Definition-scoped alternate pronunciations.
    pub fn alternative_pronunciations(
        self,
    ) -> impl ExactSizeIterator<Item = SourcedAlternativePronunciationRef<'a>> + 'a {
        self.inner
            .alternative_pronunciations
            .iter()
            .map(|inner| SourcedAlternativePronunciationRef { inner })
    }
    /// Definition-scoped classifier references.
    pub fn measure_words(
        self,
    ) -> impl ExactSizeIterator<Item = SourcedMeasureWordReferenceRef<'a>> + 'a {
        self.inner
            .measure_words
            .iter()
            .map(|inner| SourcedMeasureWordReferenceRef { inner })
    }
    /// Materializes this definition.
    pub fn to_owned(self) -> Definition {
        rkyv::deserialize::<Definition, rkyv::rancor::Error>(self.inner)
            .expect("build.rs validated the lexical archive")
    }
}

fn source(value: &ArchivedSource) -> Source {
    rkyv::deserialize::<Source, rkyv::rancor::Error>(value)
        .expect("build.rs validated the lexical archive")
}

fn chinese_variety(value: &ArchivedChineseVariety) -> ChineseVariety {
    rkyv::deserialize::<ChineseVariety, rkyv::rancor::Error>(value)
        .expect("build.rs validated the lexical archive")
}

fn qualifier_category(value: &ArchivedQualifierCategory) -> QualifierCategory {
    rkyv::deserialize::<QualifierCategory, rkyv::rancor::Error>(value)
        .expect("build.rs validated the lexical archive")
}

fn lexical_kind(value: &ArchivedLexicalKind) -> LexicalKind {
    rkyv::deserialize::<LexicalKind, rkyv::rancor::Error>(value)
        .expect("build.rs validated the lexical archive")
}

fn part_of_speech(value: &ArchivedPartOfSpeech) -> PartOfSpeech {
    rkyv::deserialize::<PartOfSpeech, rkyv::rancor::Error>(value)
        .expect("build.rs validated the lexical archive")
}

fn hsk_level(value: &ArchivedHskLevel) -> HskLevel {
    rkyv::deserialize::<HskLevel, rkyv::rancor::Error>(value)
        .expect("build.rs validated the lexical archive")
}

struct SourceSlice<'a>(&'a [ArchivedSource]);

impl Serialize for SourceSlice<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&source(value))?;
        }
        sequence.end()
    }
}

macro_rules! impl_sourced_serialize {
    ($name:ident) => {
        impl Serialize for $name<'_> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                use serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct("Sourced", 2)?;
                state.serialize_field("value", &self.value())?;
                state.serialize_field("sources", &SourceSlice(self.inner.sources.as_slice()))?;
                state.end()
            }
        }
    };
}

impl_sourced_serialize!(SourcedStringRef);
impl_sourced_serialize!(SourcedMeasureWordReferenceRef);
impl_sourced_serialize!(SourcedExampleRef);
impl_sourced_serialize!(SourcedQualifierRef);
impl_sourced_serialize!(SourcedLexicalKindRef);
impl_sourced_serialize!(SourcedPartOfSpeechRef);
impl_sourced_serialize!(SourcedAlternativePronunciationRef);

macro_rules! view_slice {
    ($name:ident, $archived:ty, $view:ident) => {
        struct $name<'a>(&'a [$archived]);

        impl Serialize for $name<'_> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                use serde::ser::SerializeSeq;
                let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
                for inner in self.0 {
                    sequence.serialize_element(&$view { inner })?;
                }
                sequence.end()
            }
        }
    };
}

view_slice!(DefinitionSlice, ArchivedDefinition, DefinitionRef);
view_slice!(
    SourcedStringSlice,
    ArchivedSourced<String>,
    SourcedStringRef
);
view_slice!(
    SourcedExampleSlice,
    ArchivedSourced<Example>,
    SourcedExampleRef
);
view_slice!(
    SourcedQualifierSlice,
    ArchivedSourced<Qualifier>,
    SourcedQualifierRef
);
view_slice!(
    SourcedLexicalKindSlice,
    ArchivedSourced<LexicalKind>,
    SourcedLexicalKindRef
);
view_slice!(
    SourcedPartOfSpeechSlice,
    ArchivedSourced<PartOfSpeech>,
    SourcedPartOfSpeechRef
);
view_slice!(
    SourcedAlternativePronunciationSlice,
    ArchivedSourced<AlternativePronunciation>,
    SourcedAlternativePronunciationRef
);
view_slice!(
    SourcedMeasureWordReferenceSlice,
    ArchivedSourced<MeasureWordReference>,
    SourcedMeasureWordReferenceRef
);

struct HskLevelSlice<'a>(&'a [ArchivedHskLevel]);

impl Serialize for HskLevelSlice<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&hsk_level(value))?;
        }
        sequence.end()
    }
}

impl Serialize for PinyinRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Pinyin", 3)?;
        state.serialize_field("marks", self.marks())?;
        state.serialize_field("numbers", self.numbers())?;
        state.serialize_field("tones", self.tones())?;
        state.end()
    }
}

impl Serialize for AlternativePronunciationRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AlternativePronunciation", 2)?;
        state.serialize_field("pronunciation", &self.pronunciation())?;
        state.serialize_field("label", self.label())?;
        state.end()
    }
}

impl Serialize for MeasureWordReferenceRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("MeasureWordReference", 4)?;
        state.serialize_field("traditional", self.traditional())?;
        state.serialize_field("simplified", self.simplified())?;
        state.serialize_field("lexical_id", &self.lexical_id())?;
        state.serialize_field(
            "varieties",
            &ChineseVarietySlice(self.inner.varieties.as_slice()),
        )?;
        state.end()
    }
}

struct ChineseVarietySlice<'a>(&'a [ArchivedChineseVariety]);

impl Serialize for ChineseVarietySlice<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&chinese_variety(value))?;
        }
        sequence.end()
    }
}

impl Serialize for ExampleRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Example", 3)?;
        state.serialize_field("simplified", &self.simplified())?;
        state.serialize_field("traditional", &self.traditional())?;
        state.serialize_field("english", &self.english())?;
        state.end()
    }
}

impl Serialize for QualifierRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Qualifier", 2)?;
        state.serialize_field("category", &self.category())?;
        state.serialize_field("value", self.value())?;
        state.end()
    }
}

impl Serialize for HskLevelsRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("HskLevels", 3)?;
        state.serialize_field("hsk_2015", &HskLevelSlice(self.inner.hsk_2015.as_slice()))?;
        state.serialize_field(
            "proficiency_standard_2021",
            &HskLevelSlice(self.inner.proficiency_standard_2021.as_slice()),
        )?;
        state.serialize_field(
            "hsk_exam_syllabus_2025",
            &HskLevelSlice(self.inner.hsk_exam_syllabus_2025.as_slice()),
        )?;
        state.end()
    }
}

impl Serialize for DefinitionRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Definition", 8)?;
        state.serialize_field("gloss", &self.gloss())?;
        state.serialize_field(
            "examples",
            &SourcedExampleSlice(self.inner.examples.as_slice()),
        )?;
        state.serialize_field(
            "commentary",
            &SourcedStringSlice(self.inner.commentary.as_slice()),
        )?;
        state.serialize_field(
            "qualifiers",
            &SourcedQualifierSlice(self.inner.qualifiers.as_slice()),
        )?;
        state.serialize_field(
            "lexical_kinds",
            &SourcedLexicalKindSlice(self.inner.lexical_kinds.as_slice()),
        )?;
        state.serialize_field(
            "parts_of_speech",
            &SourcedPartOfSpeechSlice(self.inner.parts_of_speech.as_slice()),
        )?;
        state.serialize_field(
            "alternative_pronunciations",
            &SourcedAlternativePronunciationSlice(self.inner.alternative_pronunciations.as_slice()),
        )?;
        state.serialize_field(
            "measure_words",
            &SourcedMeasureWordReferenceSlice(self.inner.measure_words.as_slice()),
        )?;
        state.end()
    }
}

impl Serialize for LexicalUnitRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("LexicalUnit", 9)?;
        state.serialize_field("id", &self.id())?;
        state.serialize_field("simplified", self.simplified())?;
        state.serialize_field("traditional", self.traditional())?;
        state.serialize_field("pinyin", &self.pinyin())?;
        state.serialize_field("commonness", &self.commonness())?;
        state.serialize_field(
            "alternative_pronunciations",
            &SourcedAlternativePronunciationSlice(self.inner.alternative_pronunciations.as_slice()),
        )?;
        state.serialize_field(
            "measure_words",
            &SourcedMeasureWordReferenceSlice(self.inner.measure_words.as_slice()),
        )?;
        state.serialize_field("hsk", &self.hsk())?;
        state.serialize_field("english", &DefinitionSlice(self.inner.english.as_slice()))?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_preimage_uses_nul_separators_and_nfc_headwords() {
        let bytes = LexicalId::prehash_bytes(" 烟火 ", "煙火", "yan1huo3").unwrap();
        assert_eq!(bytes, "烟火\0煙火\0yan1huo3".as_bytes());
        assert_eq!(
            LexicalId::new("烟火", "煙火", "yan1huo3")
                .unwrap()
                .to_string(),
            "1:1f3478580959306ec1a7c9339a95346106204a327f7d0d7ea4764cdf49cdc2d9"
        );
        assert_eq!(
            LexicalId::new("烟火", "煙火", "yan1 huo3").unwrap(),
            LexicalId::new("烟火", "煙火", "yan1-huo3").unwrap()
        );
    }

    #[test]
    fn identity_preserves_case_and_tones() {
        assert_ne!(
            LexicalId::new("复明", "復明", "fu4Ming2").unwrap(),
            LexicalId::new("复明", "復明", "fu4ming2").unwrap()
        );
        assert_ne!(
            LexicalId::new("烟火", "煙火", "yan1huo3").unwrap(),
            LexicalId::new("烟火", "煙火", "yan1huo5").unwrap()
        );
    }

    #[test]
    fn lexical_id_json_round_trip_preserves_its_representation() {
        let id = LexicalId::new("烟火", "煙火", "yan1huo3").unwrap();
        let encoded = serde_json::to_string(&id).unwrap();

        assert_eq!(encoded, serde_json::to_string(&id.to_string()).unwrap());
        assert_eq!(id, serde_json::from_str(&encoded).unwrap());
    }

    #[test]
    fn lexical_id_deserialization_rejects_invalid_values() {
        let invalid = [
            format!("2:{}", "0".repeat(64)),
            format!("1:{}", "0".repeat(63)),
            format!("1:{}A", "0".repeat(63)),
            format!("1:{}g", "0".repeat(63)),
        ];

        for value in invalid {
            let encoded = serde_json::to_string(&value).unwrap();
            let error = serde_json::from_str::<LexicalId>(&encoded).unwrap_err();
            assert!(
                error.to_string().contains("InvalidLexicalId"),
                "unexpected error for {value:?}: {error}"
            );
        }
    }

    #[test]
    fn headwords_reject_empty_values_and_controls() {
        assert_eq!(normalize_headword(" \n "), Err(ModelError::EmptyHeadword));
        assert_eq!(
            normalize_headword("烟\0火"),
            Err(ModelError::InvalidHeadword)
        );
    }

    #[test]
    fn numbered_pinyin_canonicalizes_variants_and_rejects_invalid_input() {
        assert_eq!(canonicalize_numbered_pinyin("nu:3"), Ok("nü3".to_owned()));
        assert_eq!(canonicalize_numbered_pinyin("nv3"), Ok("nü3".to_owned()));
        assert_eq!(
            canonicalize_numbered_pinyin("Ya4 dang1 · Si1 mi4"),
            Ok("Ya4dang1Si1mi4".to_owned())
        );
        for invalid in ["", "ma", "ma0", "xx5", "notpinyin5", "yi1 bu"] {
            assert_eq!(
                canonicalize_numbered_pinyin(invalid),
                Err(ModelError::InvalidPinyin),
                "accepted {invalid:?}"
            );
        }
    }
}
