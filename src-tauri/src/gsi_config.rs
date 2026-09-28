use std::path::Path;
pub const NAME: &str = "gamestate_integration_necko7.cfg";
pub fn render(token: &str, port: u16) -> String {
    format!(
        r#""Necko7 CS2 Integration"
{{
    "uri" "http://127.0.0.1:{port}/gsi"
    "timeout" "5.0"
    "buffer" "0.1"
    "throttle" "0.1"
    "heartbeat" "30.0"
    "auth" {{ "token" "{token}" }}
    "data"
    {{
        "provider" "1"
        "map" "1"
        "round" "1"
        "phase_countdowns" "1"
        "player_id" "1"
        "player_state" "1"
        "player_weapons" "1"
        "player_match_stats" "1"
    }}
}}
"#
    )
}
pub fn install(dir: &Path, token: &str, port: u16) -> Result<std::path::PathBuf, String> {
    if token.len() != 44
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
    {
        return Err("Invalid local credential; reset settings".into());
    }
    let path = dir.join(NAME);
    std::fs::write(&path, render(token, port))
        .map_err(|_| "Cannot write CS2 GSI config; check folder permissions")?;
    Ok(path)
}
pub fn remove_owned(path: &Path, token: &str, port: u16) {
    let current = render(token, port);
    let legacy = current.replace("        \"phase_countdowns\" \"1\"\n", "");
    if path.file_name().and_then(|s| s.to_str()) == Some(NAME)
        && std::fs::read_to_string(path).is_ok_and(|s| s == current || s == legacy)
    {
        let _ = std::fs::remove_file(path);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installation_is_idempotent_and_cleanup_is_owned() {
        let dir = std::env::temp_dir().join(format!("necko7-gsi-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let token = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let unrelated = dir.join("gamestate_integration_other.cfg");
        std::fs::write(&unrelated, "unrelated").unwrap();
        let path = install(&dir, token, 31337).unwrap();
        install(&dir, token, 31337).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            render(token, 31337)
        );
        remove_owned(&path, "different", 31337);
        assert!(path.exists());
        remove_owned(&path, token, 31337);
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(&unrelated).unwrap(), "unrelated");
        std::fs::remove_file(unrelated).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn config_is_valid_vdf() {
        let text = render("safe-token", 31337);
        assert!(keyvalues_parser::parse(&text).is_ok());
        assert!(text.contains("http://127.0.0.1:31337/gsi"));
        assert!(text.contains("player_match_stats"));
        assert!(text.contains("\"phase_countdowns\" \"1\""));
        for category in ["allplayers", "position", "grenades"] {
            assert!(!text.contains(category));
        }
    }
    #[test]
    fn cleanup_accepts_exact_legacy_config_but_preserves_user_edits() {
        let dir = std::env::temp_dir().join(format!("necko7-legacy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join(NAME);
        let current = render("safe-token", 31337);
        std::fs::write(&path, format!("{current}// user edit\n")).unwrap();
        remove_owned(&path, "safe-token", 31337);
        assert!(path.exists());
        std::fs::write(
            &path,
            current.replace("        \"phase_countdowns\" \"1\"\n", ""),
        )
        .unwrap();
        remove_owned(&path, "safe-token", 31337);
        assert!(!path.exists());
        std::fs::remove_dir(&dir).unwrap();
    }
}
