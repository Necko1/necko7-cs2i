use keyvalues_parser::{Value, Vdf};
use std::path::{Component, Path, PathBuf};

fn string<'a>(value: &'a Value<'_>, key: &str) -> Option<&'a str> {
    value.get_obj()?.get(key)?.first()?.get_str()
}
pub fn libraries(text: &str, root: &Path) -> Result<Vec<PathBuf>, String> {
    let doc = keyvalues_parser::parse(text)
        .map(Vdf::from)
        .map_err(|_| "Invalid Steam libraryfolders.vdf")?;
    let obj = doc.value.get_obj().ok_or("Invalid Steam libraries")?;
    let mut paths = vec![root.to_path_buf()];
    for (key, values) in obj.iter() {
        if !key.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        for value in values {
            if let Some(path) = string(value, "path").or_else(|| value.get_str()) {
                let path = PathBuf::from(path);
                if path.is_absolute() && !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }
    Ok(paths)
}
pub fn install_dir(text: &str) -> Result<String, String> {
    let doc = keyvalues_parser::parse(text)
        .map(Vdf::from)
        .map_err(|_| "Invalid appmanifest_730.acf")?;
    if string(&doc.value, "appid") != Some("730") {
        return Err("Steam manifest is not CS2".into());
    }
    let dir = string(&doc.value, "installdir").ok_or("CS2 install directory missing")?;
    if dir.is_empty()
        || dir.contains(['/', '\\', ':'])
        || !matches!(
            Path::new(dir).components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err("Unsafe CS2 install directory".into());
    }
    Ok(dir.into())
}
fn read(path: &Path) -> Result<String, String> {
    if std::fs::metadata(path)
        .map_err(|_| "Steam file not found")?
        .len()
        > 1024 * 1024
    {
        return Err("Steam file too large".into());
    }
    std::fs::read_to_string(path).map_err(|_| "Cannot read Steam file".into())
}
#[cfg(windows)]
fn roots() -> Vec<PathBuf> {
    use winreg::{enums::*, RegKey};
    let mut roots = Vec::new();
    for (hive, key, field) in [
        (HKEY_CURRENT_USER, "Software\\Valve\\Steam", "SteamPath"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Valve\\Steam", "InstallPath"),
    ] {
        for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
            if let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(key, KEY_READ | view) {
                if let Ok(value) = key.get_value::<String, _>(field) {
                    let path = PathBuf::from(value);
                    if path.is_absolute() && !roots.contains(&path) {
                        roots.push(path);
                    }
                }
            }
        }
    }
    roots
}
#[cfg(not(windows))]
fn roots() -> Vec<PathBuf> {
    Vec::new()
}
pub fn discover() -> Result<PathBuf, String> {
    for root in roots() {
        let libraries = read(&root.join("steamapps/libraryfolders.vdf"))
            .and_then(|s| libraries(&s, &root))
            .unwrap_or_else(|_| vec![root]);
        for library in libraries {
            if let Ok(dir) =
                read(&library.join("steamapps/appmanifest_730.acf")).and_then(|s| install_dir(&s))
            {
                let cfg = library
                    .join("steamapps/common")
                    .join(dir)
                    .join("game/csgo/cfg");
                if cfg.is_dir() {
                    return Ok(cfg);
                }
            }
        }
    }
    Err("CS2 not found. Install CS2 in Steam, then Retry discovery.".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifests() {
        assert_eq!(
            install_dir(include_str!("../tests/fixtures/appmanifest_730.acf")).unwrap(),
            "Counter-Strike Global Offensive"
        );
        assert!(install_dir(r#""AppState" { "appid" "730" "installdir" "../evil" }"#).is_err());
        assert!(install_dir(r#""AppState" { "appid" "440" "installdir" "CS2" }"#).is_err());
    }
    #[test]
    fn multiple_libraries() {
        let paths = libraries(
            include_str!("../tests/fixtures/libraryfolders.vdf"),
            Path::new("C:\\Steam"),
        )
        .unwrap();
        #[cfg(windows)]
        assert_eq!(paths.len(), 3);
        assert!(!paths.is_empty());
    }
}
