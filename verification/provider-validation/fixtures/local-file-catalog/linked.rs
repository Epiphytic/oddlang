
#[test]
fn actual_linked_catalog() {
    use crate::inert::{Naming, inspect_catalog};
    use sha2::{Digest, Sha256};
    use std::{fs, os::unix::fs::PermissionsExt};

    let root = std::env::temp_dir().join(format!("actual-linked-catalog-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let digest = <[u8; 32]>::from(Sha256::digest(b"x"));
    let digest: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let catalog = format!("header\nrecord 1 384 {digest}\nextra 1 384 {digest}\n");
    fs::write(root.join("catalog.txt"), &catalog).unwrap();
    for name in ["record", "extra"] {
        let path = root.join(name);
        fs::write(&path, b"x").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let result = inspect_catalog(&root, Naming::Fixed, Sha256::digest(catalog.as_bytes()).into()).unwrap();
    assert_eq!(result.family, "header");
    assert_eq!(result.items.len(), 2);
    fs::remove_dir_all(root).unwrap();
}
