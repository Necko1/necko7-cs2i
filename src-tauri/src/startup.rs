// Windows Run values are command lines, not bare executable paths. The autostart
// plugin's auto-launch 0.5 dependency does not quote them or validate their target.
#[cfg(windows)]
mod windows {
    use std::{io, path::Path};
    use winreg::{enums::*, RegKey, RegValue};

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const ENABLED: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    fn command(exe: &Path) -> io::Result<String> {
        let path = exe
            .to_str()
            .ok_or_else(|| io::Error::other("Invalid executable path"))?;
        if !exe.is_absolute() || path.contains(['"', '\0']) {
            return Err(io::Error::other("Invalid executable path"));
        }
        Ok(format!("\"{path}\" --autostart"))
    }

    fn optional<T>(result: io::Result<T>) -> io::Result<Option<T>> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    struct Registration<'a> {
        hive: RegKey,
        run: &'a str,
        approved: &'a str,
        name: &'a str,
    }

    impl Registration<'_> {
        fn value(&self) -> io::Result<Option<String>> {
            let Some(key) = optional(self.hive.open_subkey(self.run))? else {
                return Ok(None);
            };
            optional(key.get_value(self.name))
        }

        fn approved(&self) -> io::Result<bool> {
            let Some(key) = optional(self.hive.open_subkey(self.approved))? else {
                return Ok(true);
            };
            let Some(value) = optional(key.get_raw_value(self.name))? else {
                return Ok(true);
            };
            // Windows uses 2/6 for enabled and 3/7 for disabled. The timestamp
            // alone is insufficient: a disabled entry can have a zero timestamp.
            Ok(value.vtype == REG_BINARY
                && value.bytes.len() == 12
                && matches!(value.bytes[0], 2 | 6))
        }

        fn is_enabled(&self, exe: &Path) -> io::Result<bool> {
            Ok(self.value()?.as_deref() == Some(command(exe)?.as_str()) && self.approved()?)
        }

        fn set_enabled(&self, exe: &Path, enabled: bool) -> io::Result<()> {
            if enabled {
                let command = command(exe)?;
                if !exe.is_file() {
                    return Err(io::Error::other("Startup executable is missing"));
                }
                let (run, _) = self.hive.create_subkey(self.run)?;
                run.set_value(self.name, &command)?;
                let (approved, _) = self.hive.create_subkey(self.approved)?;
                approved.set_raw_value(
                    self.name,
                    &RegValue {
                        vtype: REG_BINARY,
                        bytes: ENABLED.to_vec(),
                    },
                )?;
            } else {
                if let Some(run) =
                    optional(self.hive.open_subkey_with_flags(self.run, KEY_SET_VALUE))?
                {
                    optional(run.delete_value(self.name))?;
                }
                if let Some(approved) = optional(
                    self.hive
                        .open_subkey_with_flags(self.approved, KEY_SET_VALUE),
                )? {
                    optional(approved.delete_value(self.name))?;
                }
            }
            if self.is_enabled(exe)? != enabled {
                return Err(io::Error::other(
                    "Windows startup setting could not be verified",
                ));
            }
            Ok(())
        }

        fn repair_existing(&self, exe: &Path) -> io::Result<()> {
            // Migrate only this executable's exact legacy entry. Preserve opt-out,
            // Task Manager overrides and entries belonging to another installation.
            let legacy = format!("{} --autostart", exe.display());
            if self.value()?.as_deref() == Some(legacy.as_str()) && self.approved()? {
                self.hive
                    .open_subkey_with_flags(self.run, KEY_SET_VALUE)?
                    .set_value(self.name, &command(exe)?)?;
            }
            Ok(())
        }
    }

    fn registration() -> Registration<'static> {
        Registration {
            hive: RegKey::predef(HKEY_CURRENT_USER),
            run: RUN,
            approved: APPROVED,
            name: env!("CARGO_PKG_NAME"),
        }
    }

    pub fn is_enabled() -> io::Result<bool> {
        registration().is_enabled(&std::env::current_exe()?)
    }
    pub fn set_enabled(enabled: bool) -> io::Result<()> {
        registration().set_enabled(&std::env::current_exe()?, enabled)
    }
    pub fn repair_existing() -> io::Result<()> {
        registration().repair_existing(&std::env::current_exe()?)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn quotes_spaces_and_unicode_and_rejects_invalid_paths() {
            assert_eq!(
                command(Path::new(r"C:\Users\Streamer Name\клиент\necko7-cs2i.exe")).unwrap(),
                r#""C:\Users\Streamer Name\клиент\necko7-cs2i.exe" --autostart"#
            );
            assert_eq!(
                command(Path::new(r"C:\client.exe")).unwrap(),
                r#""C:\client.exe" --autostart"#
            );
            assert!(command(Path::new("relative.exe")).is_err());
            assert!(command(Path::new("C:\\bad\"path.exe")).is_err());
        }

        #[test]
        fn windows_registry_roundtrip_migration_and_task_manager_override() {
            let root = format!(r"Software\necko7-cs2i-tests\{}", uuid::Uuid::new_v4());
            let hive = RegKey::predef(HKEY_CURRENT_USER);
            let (key, _) = hive.create_subkey(&root).unwrap();
            let registration = Registration {
                hive: key,
                run: "Run",
                approved: "Approved",
                name: "test-client",
            };
            let exe = std::env::current_exe().unwrap();
            let result = std::panic::catch_unwind(|| {
                assert!(!registration.is_enabled(&exe).unwrap());
                registration.repair_existing(&exe).unwrap();
                assert!(registration.value().unwrap().is_none());
                registration.set_enabled(&exe, false).unwrap();
                registration.set_enabled(&exe, true).unwrap();
                assert!(registration.is_enabled(&exe).unwrap());
                assert_eq!(
                    registration.value().unwrap().unwrap(),
                    command(&exe).unwrap()
                );
                let run = registration
                    .hive
                    .open_subkey_with_flags("Run", KEY_ALL_ACCESS)
                    .unwrap();
                run.set_value("unrelated", &"leave this alone").unwrap();
                run.set_value(registration.name, &r#""C:\old\client.exe" --autostart"#)
                    .unwrap();
                assert!(!registration.is_enabled(&exe).unwrap());
                registration.repair_existing(&exe).unwrap();
                assert!(registration.value().unwrap().unwrap().contains(r"C:\old"));
                let legacy = format!("{} --autostart", exe.display());
                run.set_value(registration.name, &legacy).unwrap();
                registration.repair_existing(&exe).unwrap();
                assert!(registration.is_enabled(&exe).unwrap());
                let approved = registration
                    .hive
                    .open_subkey_with_flags("Approved", KEY_ALL_ACCESS)
                    .unwrap();
                for disabled in [3, 7] {
                    let mut bytes = ENABLED;
                    bytes[0] = disabled;
                    approved
                        .set_raw_value(
                            registration.name,
                            &RegValue {
                                vtype: REG_BINARY,
                                bytes: bytes.to_vec(),
                            },
                        )
                        .unwrap();
                    assert!(!registration.is_enabled(&exe).unwrap());
                    run.set_value(registration.name, &legacy).unwrap();
                    registration.repair_existing(&exe).unwrap();
                    assert_eq!(registration.value().unwrap().unwrap(), legacy);
                }
                registration.set_enabled(&exe, true).unwrap();
                assert!(registration.is_enabled(&exe).unwrap());
                registration.set_enabled(&exe, false).unwrap();
                registration.set_enabled(&exe, false).unwrap();
                assert!(!registration.is_enabled(&exe).unwrap());
                assert_eq!(
                    run.get_value::<String, _>("unrelated").unwrap(),
                    "leave this alone"
                );
                assert!(optional(approved.get_raw_value(registration.name))
                    .unwrap()
                    .is_none());
            });
            drop(registration);
            hive.delete_subkey_all(&root).unwrap();
            result.unwrap();
        }
    }
}

pub fn is_enabled(app: &tauri::AppHandle) -> Result<bool, String> {
    #[cfg(windows)]
    {
        let _ = app;
        windows::is_enabled().map_err(|e| format!("Cannot read Windows startup setting: {e}"))
    }
    #[cfg(not(windows))]
    {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().map_err(|e| e.to_string())
    }
}

pub fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = app;
        windows::set_enabled(enabled)
            .map_err(|e| format!("Cannot change Windows startup setting: {e}"))
    }
    #[cfg(not(windows))]
    {
        use tauri_plugin_autostart::ManagerExt;
        let manager = app.autolaunch();
        if enabled {
            manager.enable()
        } else {
            manager.disable()
        }
        .map_err(|e| e.to_string())
    }
}

pub fn repair_existing() {
    #[cfg(windows)]
    if let Err(error) = windows::repair_existing() {
        tracing::warn!(%error, "Cannot repair existing Windows startup entry");
    }
}
