//! Dice content-addressed store: FastCDC + BLAKE3 + zstd.
//! Shared org-wide dedup lives in the chunk store; ownership metadata belongs in the server DB.

pub mod chunker;
pub mod codec;
pub mod crypto;
pub mod error;
pub mod exclude;
pub mod gc;
pub mod ingest;
pub mod lock;
pub mod manifest;
pub mod paths;
pub mod store;

pub use chunker::{hash_bytes, ChunkerOptions};
pub use crypto::StoreCipher;
pub use error::{DiceError, Result};
pub use exclude::ExcludeSet;
pub use gc::{extract_hashes_from_manifest_json, gc_unreferenced, measure_store_bytes, GcReport};
pub use ingest::{chunk_file_for_push, ingest_bytes, ingest_file, IngestResult};
pub use manifest::{Manifest, ManifestChunk};
pub use store::{
    assemble_bytes, clear_store_cipher, has_chunk, init_store, list_missing, set_store_cipher,
    write_chunk,
};
