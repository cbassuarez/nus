//! Opt-in real OS credential-store test. Creates and removes its own credential.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = std::env::args().nth(1) {
        let path = std::path::Path::new(&path).join("session.json");
        if std::env::args().nth(2).as_deref() == Some("locked") {
            let before = std::fs::read(&path)?;
            assert!(nus_vault::read(&path).is_err());
            assert!(nus_vault::write(&path, b"must not replace").is_err());
            assert_eq!(std::fs::read(path)?, before);
            return Ok(());
        }
        let bytes = nus_vault::read(&path)?;
        assert_eq!(bytes, b"synthetic-state-canary");
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("session.json");
    nus_vault::write(&path, b"synthetic-state-canary")?;
    let id = std::fs::read_to_string(temp.path().join(".vault-id"))?;
    let entry = keyring::Entry::new("dev.nus.local-state.v1", &id)?;
    let result = std::process::Command::new(std::env::current_exe()?)
        .arg(temp.path())
        .status();
    entry.delete_credential()?;
    assert!(
        result?.success(),
        "another process could not decrypt the synthetic state"
    );
    assert!(std::process::Command::new(std::env::current_exe()?)
        .arg(temp.path())
        .arg("locked")
        .status()?
        .success());
    assert!(!std::fs::read(path)?.windows(9).any(|w| w == b"synthetic"));
    println!("PASS real OS keychain: encrypted disk bytes, cross-process read, missing-key fail-closed preservation, test credential removed");
    Ok(())
}
