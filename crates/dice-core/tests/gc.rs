use std::collections::HashSet;
use std::fs;

use dice_core::{gc_unreferenced, ingest_bytes, measure_store_bytes, ChunkerOptions};

#[test]
fn gc_removes_orphan_chunks_after_no_live_refs() {
    let dir = std::env::temp_dir().join(format!("dice-core-gc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let a = ingest_bytes(&dir, b"alpha-data-aaaaaaaa", "a.txt", ChunkerOptions::default(), 1).unwrap();
    let _b = ingest_bytes(&dir, b"beta-data-bbbbbbbbb", "b.txt", ChunkerOptions::default(), 1).unwrap();
    assert!(measure_store_bytes(&dir).unwrap() > 0);

    // Keep only hashes from file A live → B's chunks should be GC'd if distinct
    let mut live = HashSet::new();
    for c in &a.manifest.chunks {
        live.insert(c.hash.clone());
    }
    let report = gc_unreferenced(&dir, &live).unwrap();
    assert!(report.chunks_deleted >= 1 || a.manifest.hash == _b.manifest.hash);

    let _ = fs::remove_dir_all(&dir);
}
