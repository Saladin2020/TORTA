use dice_core::{assemble_bytes, ingest_bytes, ChunkerOptions};
use std::fs;

#[test]
fn ingest_and_assemble_roundtrip() {
    let dir = std::env::temp_dir().join(format!("dice-core-rt-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let payload = b"hello torta dice shared dedup store - roundtrip";
    let result = ingest_bytes(&dir, payload, "docs/hello.txt", ChunkerOptions::default(), 1).unwrap();
    assert_eq!(result.raw_bytes, payload.len() as u64);
    assert!(!result.manifest.hash.is_empty());

    let restored = assemble_bytes(&dir, &result.manifest).unwrap();
    assert_eq!(restored, payload);

    // Second ingest of same bytes should reuse chunks
    let again = ingest_bytes(&dir, payload, "docs/hello-copy.txt", ChunkerOptions::default(), 1).unwrap();
    assert_eq!(again.new_chunks, 0);
    assert!(again.reused_chunks > 0 || payload.is_empty());

    let _ = fs::remove_dir_all(&dir);
}
