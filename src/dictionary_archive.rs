//! Schema-4 zero-copy dictionary archive contract.
//!
//! This module is intentionally mirrored in the generator crate. The shared
//! fixture tests protect the contract without making the published consumer
//! depend on the generator.

use crate::model::{LexicalUnit, IDENTITY_VERSION, SCHEMA_VERSION};
use rkyv::{
    collections::swiss_table::{ArchivedHashMap, HashMapResolver},
    rancor::{Fallible, Source},
    ser::{Allocator, Writer},
    Archive, Place, Serialize,
};

pub(crate) const IDENTITY_LOAD_FACTOR: (usize, usize) = (7, 8);
#[allow(dead_code)]
pub(crate) const ARCHIVE_CONTRACT: &[u8; 34] = b"syng-dictionary-rkyv-v4-le-a16-p32";

#[derive(Clone, Copy, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize)]
pub(crate) struct PostingRange {
    pub(crate) start: u32,
    pub(crate) len: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize)]
pub(crate) struct ChinesePostings {
    pub(crate) simplified: PostingRange,
    pub(crate) traditional: PostingRange,
}

pub(crate) struct IdentityMap {
    entries: Vec<([u8; 32], u32)>,
}

impl IdentityMap {
    #[allow(dead_code)]
    pub(crate) fn new(entries: Vec<([u8; 32], u32)>) -> Self {
        debug_assert!(entries.windows(2).all(|pair| pair[0].0 < pair[1].0));
        Self { entries }
    }
}

impl Archive for IdentityMap {
    type Archived = ArchivedHashMap<[u8; 32], rkyv::Archived<u32>>;
    type Resolver = HashMapResolver;

    fn resolve(&self, resolver: Self::Resolver, out: Place<Self::Archived>) {
        ArchivedHashMap::resolve_from_len(self.entries.len(), IDENTITY_LOAD_FACTOR, resolver, out);
    }
}

impl<S> Serialize<S> for IdentityMap
where
    S: Fallible + Writer + Allocator + ?Sized,
    S::Error: Source,
{
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        ArchivedHashMap::<[u8; 32], rkyv::Archived<u32>>::serialize_from_iter::<
            _,
            _,
            _,
            [u8; 32],
            u32,
            S,
        >(
            self.entries
                .iter()
                .map(|(digest, runtime_key)| (digest, runtime_key)),
            IDENTITY_LOAD_FACTOR,
            serializer,
        )
    }
}

#[derive(rkyv::Archive, rkyv::Serialize)]
#[allow(dead_code)]
pub(crate) struct DictionaryArchive {
    pub(crate) archive_contract: [u8; 34],
    pub(crate) schema_version: u32,
    pub(crate) identity_version: u8,
    pub(crate) lexical_units: Vec<LexicalUnit>,
    pub(crate) identities: IdentityMap,
    pub(crate) chinese_fst: Vec<u8>,
    pub(crate) chinese_postings: Vec<ChinesePostings>,
    pub(crate) chinese_runtime_keys: Vec<u32>,
    pub(crate) pinyin_fst: Vec<u8>,
    pub(crate) pinyin_postings: Vec<PostingRange>,
    pub(crate) pinyin_runtime_keys: Vec<u32>,
}

impl DictionaryArchive {
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self::documentation_fixture(Vec::new())
    }

    /// Creates the empty archive used only while building API documentation.
    #[allow(dead_code)]
    pub(crate) fn documentation_fixture(empty_fst: Vec<u8>) -> Self {
        Self {
            archive_contract: *ARCHIVE_CONTRACT,
            schema_version: SCHEMA_VERSION,
            identity_version: IDENTITY_VERSION,
            lexical_units: Vec::new(),
            identities: IdentityMap::new(Vec::new()),
            chinese_fst: empty_fst.clone(),
            chinese_postings: Vec::new(),
            chinese_runtime_keys: Vec::new(),
            pinyin_fst: empty_fst,
            pinyin_postings: Vec::new(),
            pinyin_runtime_keys: Vec::new(),
        }
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn empty_contract_fixture_is_stable() {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&DictionaryArchive::empty()).unwrap();
        assert_eq!(bytes.len(), 112);
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            "fb10c68ad48c35314b06dc816ec72ffe96d694b23d53deb8e3849c65a73200ca"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_contract_archive_round_trips_through_validation() {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&DictionaryArchive::empty()).unwrap();
        let archived =
            rkyv::access::<ArchivedDictionaryArchive, rkyv::rancor::Error>(&bytes).unwrap();
        assert_eq!(archived.archive_contract, *ARCHIVE_CONTRACT);
        assert_eq!(archived.schema_version.to_native(), SCHEMA_VERSION);
        assert_eq!(archived.identity_version, IDENTITY_VERSION);
        assert!(archived.identities.is_empty());
    }
}
