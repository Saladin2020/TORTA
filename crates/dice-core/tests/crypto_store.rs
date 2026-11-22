use dice_core::{
    assemble_bytes, clear_store_cipher, ingest_bytes, set_store_cipher, ChunkerOptions, StoreCipher,
};
use std::fs;

#[test]
fn encrypted_store_roundtrip_preserves_dedup() {
    let dir = std::env::temp_dir().join(format!("dice-enc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let cipher = StoreCipher::from_hex(
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    )
    .unwrap();
    set_store_cipher(Some(cipher));

    let payload = b"org-shared-dedup-with-encryption";
    let a = ingest_bytes(&dir, payload, "a.bin", ChunkerOptions::default(), 1).unwrap();
    let b = ingest_bytes(&dir, payload, "b.bin", ChunkerOptions::default(), 1).unwrap();
    assert_eq!(b.new_chunks, 0);
    assert!(b.reused_chunks > 0);

    let restored = assemble_bytes(&dir, &a.manifest).unwrap();
    assert_eq!(restored, payload);

    clear_store_cipher();
    let _ = fs::remove_dir_all(&dir);
}
