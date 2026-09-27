use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

#[cfg(windows)]
pub fn load(existing_device: bool) -> Result<SigningKey, String> {
    use zeroize::Zeroizing;
    let entry = keyring::Entry::new("moe.necko7.desktop", "ed25519")
        .map_err(|_| "Cannot open Windows Credential Manager")?;
    match entry.get_secret() {
        Ok(secret) => {
            let secret = Zeroizing::new(secret);
            let bytes: &[u8;32] = secret.as_slice().try_into().map_err(|_| "Device credential is corrupted. Unpair in the dashboard, then reset identity in Settings.")?;
            Ok(SigningKey::from_bytes(bytes))
        }
        Err(keyring::Error::NoEntry) if !existing_device => {
            let key = SigningKey::generate(&mut OsRng);
            entry.set_secret(&key.to_bytes()).map_err(|_| "Cannot save device credential")?;
            Ok(key)
        }
        Err(_) => Err("Device credential is unavailable. Unlock Windows Credential Manager or unpair in the dashboard and reset identity.".into()),
    }
}
#[cfg(not(windows))]
pub fn load(_: bool) -> Result<SigningKey, String> {
    Err("Protected device identity requires Windows".into())
}

#[cfg(windows)]
pub fn reset() -> Result<SigningKey, String> {
    let key = SigningKey::generate(&mut OsRng);
    keyring::Entry::new("moe.necko7.desktop", "ed25519")
        .map_err(|_| "Credential store unavailable")?
        .set_secret(&key.to_bytes())
        .map_err(|_| "Cannot save credential")?;
    Ok(key)
}
#[cfg(not(windows))]
pub fn reset() -> Result<SigningKey, String> {
    load(false)
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    #[ignore = "writes and deletes a unique test entry in Windows Credential Manager"]
    fn windows_credential_roundtrip() {
        let account = format!("test-{}", uuid::Uuid::new_v4());
        let entry = keyring::Entry::new("moe.necko7.desktop.tests", &account).unwrap();
        let key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        entry.set_secret(&key.to_bytes()).unwrap();
        let recovered = entry.get_secret();
        entry.delete_credential().unwrap();
        assert_eq!(recovered.unwrap(), key.to_bytes());
        assert!(matches!(entry.get_secret(), Err(keyring::Error::NoEntry)));
    }
}
