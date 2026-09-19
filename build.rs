use fst::{IntoStreamer, Map, Streamer};
use rkyv::util::AlignedVec;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

#[allow(dead_code)]
#[path = "src/dictionary_archive.rs"]
mod dictionary_archive;
#[path = "src/english_search_format.rs"]
mod english_search_format;
#[allow(dead_code)]
#[path = "src/model.rs"]
mod model;

use dictionary_archive::{
    ArchivedDictionaryArchive, ArchivedPostingRange, DictionaryArchive, ARCHIVE_CONTRACT,
};
use english_search_format::{
    write_container, EnglishSearchIndex, Section, SectionCodec, SectionKind, TextKey, TokenId,
    GRAMMAR_VERSION, MORPHOLOGY_VERSION, NORMALIZATION_VERSION, SEARCH_FORMAT_VERSION,
};
use model::{IDENTITY_VERSION, SCHEMA_VERSION};

const BUNDLE_VERSION: &str = "4.1.0";
const DATA_ENV: &str = "CHINESE_DICTIONARY_DATA_DIR";
const RELEASE_URL: &str = "https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/chinese_dictionary-data-4.1.0.tar.gz";
const ATTRIBUTION_URL: &str = "https://github.com/sotch-pr35mac/chinese_dictionary/releases/download/v4.1.0/wiktionary-attribution-4.1.0.json";
const REQUIRED_FILES: &[&str] = &[
    "dictionary.rkyv.zst",
    "english.search.zst",
    "wiktionary-attribution.json",
    "manifest.json",
    "NOTICE.md",
    "LICENSE-DATA.txt",
    "LICENSE-WORDNET.txt",
];
const ARCHIVES: &[(&str, &str)] = &[
    ("dictionary.rkyv.zst", "dictionary.rkyv"),
    ("english.search.zst", "english.search"),
];
const MEBIBYTE: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct Manifest {
    bundle_version: String,
    release_url: String,
    attribution_url: String,
    attribution_sha256: String,
    schema_version: u32,
    archive_contract: String,
    file_checksums: BTreeMap<String, String>,
    english_search: EnglishMetadata,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedManifest {
    bundle_version: String,
    bundle: ReleaseAsset,
    standalone_attribution: ExternalFile,
    files: BTreeMap<String, FileIntegrity>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseAsset {
    file_name: String,
    url: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalFile {
    file_name: String,
    url: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileIntegrity {
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct EnglishMetadata {
    compressed_bytes: u64,
    uncompressed_bytes: u64,
    compressed_sha256: String,
    uncompressed_sha256: String,
}

fn main() {
    if let Err(error) = run() {
        panic!("chinese_dictionary {BUNDLE_VERSION} data bundle validation failed: {error}");
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed={DATA_ENV}");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    println!("cargo:rerun-if-changed=data-manifest.json");

    let output_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?);
    if env::var_os("DOCS_RS").is_some() {
        return write_documentation_fixtures(&output_dir);
    }

    let trusted: TrustedManifest = serde_json::from_str(include_str!("data-manifest.json"))?;
    validate_trusted_manifest(&trusted)?;
    let data_dir = env::var_os(DATA_ENV)
        .map(PathBuf::from)
        .ok_or_else(missing_data_message)?;

    let manifest_path = data_dir.join("manifest.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let manifest_bytes = fs::read(&manifest_path).map_err(|error| {
        format!(
            "required bundle file {} is unavailable: {error}. Download {RELEASE_URL}",
            manifest_path.display()
        )
    })?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.bundle_version != BUNDLE_VERSION {
        return Err(format!(
            "bundle version mismatch: expected {BUNDLE_VERSION}, found {}",
            manifest.bundle_version
        )
        .into());
    }
    if manifest.release_url != RELEASE_URL || manifest.attribution_url != ATTRIBUTION_URL {
        return Err("bundle release URLs do not match the trusted crate manifest".into());
    }
    if manifest.attribution_sha256 != trusted.files["wiktionary-attribution.json"].sha256 {
        return Err("bundle attribution checksum does not match the trusted crate manifest".into());
    }
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(format!("unsupported bundle schema {}", manifest.schema_version).into());
    }
    if manifest.archive_contract.as_bytes() != ARCHIVE_CONTRACT {
        return Err("dictionary archive contract mismatch".into());
    }
    validate_external_files(&data_dir, &trusted)?;

    if manifest.file_checksums.len() != REQUIRED_FILES.len() - 1 {
        return Err("bundle manifest does not list exactly the required data files".into());
    }
    for &name in REQUIRED_FILES {
        if name == "manifest.json" {
            continue;
        }
        let expected = manifest
            .file_checksums
            .get(name)
            .ok_or_else(|| format!("bundle manifest is missing the checksum for {name}"))?;
        if expected != &trusted.files[name].sha256 {
            return Err(format!("bundle manifest has the wrong checksum for {name}").into());
        }
    }

    for &(archive, output) in ARCHIVES {
        let compressed = fs::read(data_dir.join(archive))?;
        let maximum = match archive {
            "dictionary.rkyv.zst" => 256 * MEBIBYTE,
            "english.search.zst" => manifest.english_search.uncompressed_bytes,
            _ => return Err(format!("no decompression limit for {archive}").into()),
        };
        let raw = decompress_bounded(archive, &compressed, maximum)?;
        if archive == "english.search.zst" {
            validate_english_metadata(&manifest.english_search, &compressed, &raw)?;
        }
        let temporary = output_dir.join(format!("{output}.tmp"));
        fs::write(&temporary, raw)?;
        let destination = output_dir.join(output);
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        fs::rename(temporary, destination)?;
    }
    let dictionary_bytes = fs::read(output_dir.join("dictionary.rkyv"))?;
    let mut aligned = AlignedVec::<16>::with_capacity(dictionary_bytes.len());
    aligned.extend_from_slice(&dictionary_bytes);
    let dictionary = rkyv::access::<ArchivedDictionaryArchive, rkyv::rancor::Error>(&aligned)?;
    let unit_count = validate_dictionary_archive(dictionary)?;
    let english = fs::read(output_dir.join("english.search"))?;
    let parsed = EnglishSearchIndex::parse(&english)?;
    if parsed.lexical_unit_count() != unit_count {
        return Err("English lexical-unit count mismatch".into());
    }
    validate_english_index(&parsed, unit_count)?;
    Ok(())
}

fn missing_data_message() -> String {
    format!(
        "{DATA_ENV} is not set. Download and extract:\n  {RELEASE_URL}\n\nThen configure the extracted directory, for example:\n  {DATA_ENV}=/absolute/path/to/chinese_dictionary-data-{BUNDLE_VERSION} cargo build\n\nOr persist it in .cargo/config.toml:\n  [env]\n  {DATA_ENV} = {{ value = \"vendor/chinese_dictionary-data-{BUNDLE_VERSION}\", relative = true }}"
    )
}

fn validate_trusted_manifest(trusted: &TrustedManifest) -> Result<(), Box<dyn std::error::Error>> {
    if trusted.bundle_version != BUNDLE_VERSION || env!("CARGO_PKG_VERSION") != BUNDLE_VERSION {
        return Err("crate and trusted bundle versions do not agree".into());
    }
    if trusted.bundle.file_name != format!("chinese_dictionary-data-{BUNDLE_VERSION}.tar.gz")
        || trusted.bundle.url != RELEASE_URL
        || trusted.bundle.bytes == 0
        || trusted.bundle.sha256.len() != 64
    {
        return Err("trusted bundle asset metadata is invalid".into());
    }
    if trusted.standalone_attribution.file_name
        != format!("wiktionary-attribution-{BUNDLE_VERSION}.json")
        || trusted.standalone_attribution.url != ATTRIBUTION_URL
    {
        return Err("trusted standalone attribution metadata is invalid".into());
    }
    if trusted.files.len() != REQUIRED_FILES.len()
        || REQUIRED_FILES
            .iter()
            .any(|name| !trusted.files.contains_key(*name))
    {
        return Err("trusted manifest does not contain exactly the required bundle files".into());
    }
    let attribution = &trusted.files["wiktionary-attribution.json"];
    if attribution.bytes != trusted.standalone_attribution.bytes
        || attribution.sha256 != trusted.standalone_attribution.sha256
    {
        return Err(
            "trusted standalone attribution metadata does not match the bundled file".into(),
        );
    }
    Ok(())
}

fn validate_external_files(
    data_dir: &std::path::Path,
    trusted: &TrustedManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    for &name in REQUIRED_FILES {
        let path = data_dir.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let expected = &trusted.files[name];
        let metadata = fs::metadata(&path).map_err(|error| {
            format!(
                "required bundle file {} is unavailable: {error}. Download {RELEASE_URL}",
                path.display()
            )
        })?;
        if metadata.len() != expected.bytes {
            return Err(format!(
                "length mismatch for {name}: expected {} bytes, found {}",
                expected.bytes,
                metadata.len()
            )
            .into());
        }
        let actual = sha256_file(&path)?;
        if actual != expected.sha256 {
            return Err(format!(
                "checksum mismatch for {name}: expected {}, found {actual}",
                expected.sha256
            )
            .into());
        }
    }
    Ok(())
}

fn sha256_file(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn write_documentation_fixtures(
    output_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let empty_map = Map::from_iter(std::iter::empty::<(&str, u64)>())?;
    let empty_fst = empty_map.as_fst().as_bytes().to_vec();
    let dictionary = DictionaryArchive::documentation_fixture(empty_fst.clone());
    let dictionary = rkyv::to_bytes::<rkyv::rancor::Error>(&dictionary)?;
    fs::write(output_dir.join("dictionary.rkyv"), &dictionary)?;

    let mut metadata = Vec::new();
    for value in [
        SEARCH_FORMAT_VERSION,
        NORMALIZATION_VERSION,
        GRAMMAR_VERSION,
        MORPHOLOGY_VERSION,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ] {
        metadata.extend_from_slice(&value.to_le_bytes());
    }
    metadata.extend_from_slice(&0_u32.to_le_bytes());
    metadata.extend_from_slice(&0_u32.to_le_bytes());
    metadata.extend_from_slice(&0_u32.to_le_bytes());

    let sections = (1..=13)
        .map(|raw| {
            let kind = SectionKind::try_from(raw)?;
            let bytes = match kind {
                SectionKind::TokenFst | SectionKind::PhraseFst | SectionKind::MorphologyFst => {
                    empty_fst.clone()
                }
                SectionKind::Metadata => metadata.clone(),
                _ => Vec::new(),
            };
            Ok(Section {
                kind,
                codec: SectionCodec::RawV1,
                item_count: 0,
                bytes,
            })
        })
        .collect::<Result<Vec<_>, english_search_format::FormatError>>()?;
    let english = write_container(0, sections)?;
    fs::write(output_dir.join("english.search"), english)?;
    Ok(())
}

fn decompress_bounded(
    name: &str,
    compressed: &[u8],
    maximum: u64,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let decoder = zstd::stream::read::Decoder::new(compressed)?;
    let mut limited = decoder.take(maximum + 1);
    let mut raw = Vec::new();
    limited.read_to_end(&mut raw)?;
    if raw.len() as u64 > maximum {
        return Err(format!("{name} exceeds its {maximum}-byte decompression limit").into());
    }
    Ok(raw)
}

fn validate_dictionary_archive(
    dictionary: &ArchivedDictionaryArchive,
) -> Result<u32, Box<dyn std::error::Error>> {
    if dictionary.archive_contract != *ARCHIVE_CONTRACT
        || dictionary.schema_version.to_native() != SCHEMA_VERSION
        || dictionary.identity_version != IDENTITY_VERSION
    {
        return Err("dictionary archive metadata mismatch".into());
    }
    let unit_count = u32::try_from(dictionary.lexical_units.len())?;
    if dictionary.identities.len() != dictionary.lexical_units.len() {
        return Err("identity index length mismatch".into());
    }
    for (runtime_key, unit) in dictionary.lexical_units.iter().enumerate() {
        let commonness = unit.commonness.to_native();
        if !commonness.is_finite() || commonness < 0.0 {
            return Err("lexical unit has an invalid commonness score".into());
        }
        let indexed_key = dictionary
            .identities
            .get(&unit.id.0)
            .ok_or("lexical identity is absent from the identity map")?
            .to_native();
        if indexed_key != u32::try_from(runtime_key)? {
            return Err("identity index points to the wrong lexical unit".into());
        }
        for classifier in unit.measure_words.iter().chain(
            unit.english
                .iter()
                .flat_map(|definition| definition.measure_words.iter()),
        ) {
            let Some(identity) = classifier.value.lexical_id.as_ref() else {
                continue;
            };
            let classifier_key = dictionary
                .identities
                .get(&identity.0)
                .ok_or("classifier identity is absent from the identity map")?
                .to_native();
            let classifier_unit = dictionary
                .lexical_units
                .get(usize::try_from(classifier_key)?)
                .ok_or("classifier identity points outside lexical units")?;
            if classifier_unit.traditional.as_str() != classifier.value.traditional.as_str()
                || classifier_unit.simplified.as_str() != classifier.value.simplified.as_str()
            {
                return Err("classifier identity does not match reference forms".into());
            }
        }
    }

    let chinese_fst = Map::new(dictionary.chinese_fst.as_slice())?;
    validate_fst_ordinals(&chinese_fst, dictionary.chinese_postings.len(), "Chinese")?;
    for postings in dictionary.chinese_postings.iter() {
        if postings.simplified.len.to_native() == 0 && postings.traditional.len.to_native() == 0 {
            return Err("Chinese term has no postings".into());
        }
        validate_posting_range(
            &postings.simplified,
            dictionary.chinese_runtime_keys.as_slice(),
            unit_count,
            "simplified",
            true,
        )?;
        validate_posting_range(
            &postings.traditional,
            dictionary.chinese_runtime_keys.as_slice(),
            unit_count,
            "traditional",
            true,
        )?;
    }

    let pinyin_fst = Map::new(dictionary.pinyin_fst.as_slice())?;
    validate_fst_ordinals(&pinyin_fst, dictionary.pinyin_postings.len(), "Pinyin")?;
    for range in dictionary.pinyin_postings.iter() {
        validate_posting_range(
            range,
            dictionary.pinyin_runtime_keys.as_slice(),
            unit_count,
            "Pinyin",
            false,
        )?;
    }
    Ok(unit_count)
}

fn validate_fst_ordinals<D: AsRef<[u8]>>(
    fst: &Map<D>,
    posting_count: usize,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if fst.len() != posting_count {
        return Err(format!("{name} FST/postings length mismatch").into());
    }
    let mut seen = vec![false; posting_count];
    let mut stream = fst.stream();
    while let Some((key, ordinal)) = stream.next() {
        if key.is_empty() {
            return Err(format!("empty {name} FST key").into());
        }
        let slot = seen
            .get_mut(usize::try_from(ordinal)?)
            .ok_or_else(|| format!("{name} FST ordinal is out of range"))?;
        if std::mem::replace(slot, true) {
            return Err(format!("duplicate {name} FST ordinal").into());
        }
    }
    if seen.iter().any(|value| !value) {
        return Err(format!("missing {name} FST ordinal").into());
    }
    Ok(())
}

fn validate_posting_range(
    range: &ArchivedPostingRange,
    runtime_keys: &[rkyv::Archived<u32>],
    unit_count: u32,
    name: &str,
    allow_empty: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = usize::try_from(range.start.to_native())?;
    let len = usize::try_from(range.len.to_native())?;
    let end = start.checked_add(len).ok_or("posting range overflow")?;
    let postings = runtime_keys
        .get(start..end)
        .ok_or_else(|| format!("{name} posting range is out of bounds"))?;
    if postings.is_empty() && !allow_empty {
        return Err(format!("empty {name} posting range").into());
    }
    let mut previous = None;
    for runtime_key in postings {
        let runtime_key = runtime_key.to_native();
        if runtime_key >= unit_count {
            return Err(format!("out-of-range {name} runtime key").into());
        }
        if previous.is_some_and(|value| value >= runtime_key) {
            return Err(format!("unsorted {name} postings").into());
        }
        previous = Some(runtime_key);
    }
    Ok(())
}

fn validate_english_index(
    index: &EnglishSearchIndex<'_>,
    unit_count: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = index.metadata()?;
    for raw_id in 0..metadata.token_count {
        let token_id = TokenId::new(raw_id);
        index.token_string(token_id)?;
        let token = index.token(token_id)?;
        let mut occurrence_count = 0_u32;
        for occurrence in token.occurrences()? {
            let occurrence = occurrence?;
            if occurrence.text_key.get() >= metadata.text_count {
                return Err("English occurrence text is out of range".into());
            }
            occurrence_count += 1;
        }
        if occurrence_count != token.occurrence_count {
            return Err("English occurrence count mismatch".into());
        }
        let mut hit_count = 0_u32;
        for hit in token.direct_hits() {
            if hit?.runtime_key.get() >= unit_count {
                return Err("English direct hit is out of range".into());
            }
            hit_count += 1;
        }
        if hit_count != token.hit_count {
            return Err("English direct-hit count mismatch".into());
        }
    }
    for raw_key in 0..metadata.text_count {
        let text = index.text(TextKey::new(raw_key))?;
        if text.tokens().collect::<Result<Vec<_>, _>>()?.len() != text.token_count as usize {
            return Err("English text token count mismatch".into());
        }
        for binding in text.bindings() {
            if binding?.runtime_key.get() >= unit_count {
                return Err("English text binding is out of range".into());
            }
        }
    }
    let token_fst = index.token_fst()?;
    let mut token_stream = token_fst.into_stream();
    while let Some((_key, value)) = token_stream.next() {
        if value >= u64::from(metadata.token_count) {
            return Err("English token FST value is out of range".into());
        }
    }
    let phrase_fst = index.phrase_fst()?;
    let mut phrase_stream = phrase_fst.into_stream();
    while let Some((_key, value)) = phrase_stream.next() {
        if value >= u64::from(metadata.text_count) {
            return Err("English phrase FST value is out of range".into());
        }
    }
    let families = index.morphology_families().collect::<Result<Vec<_>, _>>()?;
    if families.len() != metadata.morphology_family_count as usize {
        return Err("English morphology family count mismatch".into());
    }
    for family in &families {
        for token in family.corpus_tokens() {
            if token?.get() >= metadata.token_count {
                return Err("English morphology token is out of range".into());
            }
        }
    }
    let morphology_fst = index.morphology_fst()?;
    let mut morphology_stream = morphology_fst.into_stream();
    while let Some((surface, _offset)) = morphology_stream.next() {
        let surface = std::str::from_utf8(surface)?;
        let analyses = index
            .morphology_analyses(surface)?
            .ok_or("morphology FST surface has no analyses")?;
        for family in analyses {
            if family?.get() >= metadata.morphology_family_count {
                return Err("English morphology analysis is out of range".into());
            }
        }
    }
    Ok(())
}

fn validate_english_metadata(
    metadata: &EnglishMetadata,
    compressed: &[u8],
    raw: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    if metadata.compressed_bytes != compressed.len() as u64
        || metadata.uncompressed_bytes != raw.len() as u64
    {
        return Err("English artifact length mismatch".into());
    }
    if metadata.compressed_sha256 != format!("{:x}", Sha256::digest(compressed))
        || metadata.uncompressed_sha256 != format!("{:x}", Sha256::digest(raw))
    {
        return Err("English artifact digest mismatch".into());
    }
    Ok(())
}
