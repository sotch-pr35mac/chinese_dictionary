use bincode::Options;
use fst::{IntoStreamer, Set, Streamer};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[path = "src/english_search_format.rs"]
mod english_search_format;
#[allow(dead_code)]
#[path = "src/model.rs"]
mod model;

use english_search_format::{EnglishSearchIndex, TextKey, TokenId};
use model::{BinaryEnvelope, LexicalId, LexicalUnit, SCHEMA_VERSION};

type Data = BTreeMap<u32, LexicalUnit>;
type SearchIndex = BTreeMap<String, Vec<u32>>;
type IdentityIndex = BTreeMap<LexicalId, u32>;

const ARCHIVES: &[(&str, &str)] = &[
    ("data.dictionary.zst", "data.dictionary"),
    ("simplified.dictionary.zst", "simplified.dictionary"),
    ("traditional.dictionary.zst", "traditional.dictionary"),
    ("pinyin.dictionary.zst", "pinyin.dictionary"),
    ("identity.dictionary.zst", "identity.dictionary"),
    ("english.search.zst", "english.search"),
];
const MEBIBYTE: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    file_checksums: BTreeMap<String, String>,
    english_search: EnglishMetadata,
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
        panic!("schema-5 dictionary bundle validation failed: {error}");
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = PathBuf::from("data");
    let manifest_path = data_dir.join("manifest.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let manifest: Manifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(format!("unsupported bundle schema {}", manifest.schema_version).into());
    }

    for (name, expected) in &manifest.file_checksums {
        let path = data_dir.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = fs::read(&path)?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if &actual != expected {
            return Err(format!("checksum mismatch for {name}").into());
        }
    }

    let output_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?);
    for &(archive, output) in ARCHIVES {
        let compressed = fs::read(data_dir.join(archive))?;
        let maximum = match archive {
            "data.dictionary.zst" => 128 * MEBIBYTE,
            "identity.dictionary.zst" => 24 * MEBIBYTE,
            "pinyin.dictionary.zst" => 24 * MEBIBYTE,
            "simplified.dictionary.zst" | "traditional.dictionary.zst" => 8 * MEBIBYTE,
            "english.search.zst" => manifest.english_search.uncompressed_bytes,
            _ => return Err(format!("no decompression limit for {archive}").into()),
        };
        let raw = decompress_bounded(archive, &compressed, maximum)?;
        if archive == "english.search.zst" {
            validate_english_metadata(&manifest.english_search, &compressed, &raw)?;
        }
        let temporary = output_dir.join(format!("{output}.tmp"));
        fs::write(&temporary, raw)?;
        fs::rename(temporary, output_dir.join(output))?;
    }
    fs::copy(data_dir.join("chinese.fst"), output_dir.join("chinese.fst"))?;

    let data: Data = decode_envelope(&output_dir.join("data.dictionary"))?;
    if data.keys().copied().ne(0..u32::try_from(data.len())?) {
        return Err("runtime keys are not contiguous".into());
    }
    let unit_count = u32::try_from(data.len())?;
    for name in ["simplified", "traditional", "pinyin"] {
        let index: SearchIndex = decode_envelope(&output_dir.join(format!("{name}.dictionary")))?;
        validate_index(name, &index, unit_count)?;
    }
    let identities: IdentityIndex = decode_envelope(&output_dir.join("identity.dictionary"))?;
    if identities.len() != data.len() {
        return Err("identity index length mismatch".into());
    }
    for (identity, &runtime_key) in &identities {
        if data.get(&runtime_key).map(|unit| &unit.id) != Some(identity) {
            return Err("identity index points to the wrong lexical unit".into());
        }
    }
    for unit in data.values() {
        for classifier in unit.measure_words.iter().chain(
            unit.english
                .iter()
                .flat_map(|definition| &definition.measure_words),
        ) {
            if !identities.contains_key(&classifier.value) {
                return Err(format!("unresolved classifier identity {}", classifier.value).into());
            }
        }
    }
    let chinese = fs::read(output_dir.join("chinese.fst"))?;
    Set::new(chinese)?;
    let english = fs::read(output_dir.join("english.search"))?;
    let parsed = EnglishSearchIndex::parse(&english)?;
    if parsed.lexical_unit_count() != unit_count {
        return Err("English lexical-unit count mismatch".into());
    }
    validate_english_index(&parsed, unit_count)?;
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

fn bincode_options() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
}

fn decode_envelope<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<T, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let envelope: BinaryEnvelope<T> = bincode_options().deserialize(&bytes)?;
    if envelope.schema_version != SCHEMA_VERSION {
        return Err("dictionary envelope schema mismatch".into());
    }
    Ok(envelope.payload)
}

fn validate_index(
    name: &str,
    index: &SearchIndex,
    unit_count: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    for (key, ids) in index {
        if key.is_empty() || ids.is_empty() {
            return Err(format!("empty {name} index record").into());
        }
        if ids.iter().any(|id| *id >= unit_count) {
            return Err(format!("out-of-range {name} reference").into());
        }
        if ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(format!("unsorted {name} postings").into());
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
