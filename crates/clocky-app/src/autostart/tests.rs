use super::*;

#[test]
fn fake_follows_enable_and_disable() {
    let fake = Fake::default();
    assert!(!fake.is_enabled().unwrap());
    assert!(set(&fake, true).unwrap());
    assert!(fake.is_enabled().unwrap());
    assert!(!set(&fake, false).unwrap());
}

#[test]
fn desktop_entry_quotes_and_escapes_the_exe() {
    let text = desktop_entry(Path::new("/opt/my apps/clocky"));
    assert!(text.starts_with("[Desktop Entry]\n"), "{text}");
    assert!(text.contains("\nType=Application\n"), "{text}");
    assert!(text.contains("\nName=Clocky\n"), "{text}");
    assert!(text.contains("\nExec=\"/opt/my apps/clocky\"\n"), "{text}");
    let odd = desktop_entry(Path::new(r#"/x/a"b$c`d\e%f"#));
    assert!(odd.contains(r#"Exec="/x/a\\"b\\$c\\`d\\\\e%%f""#), "{odd}");
}

#[test]
fn launch_agent_runs_the_exe_at_login() {
    let text = launch_agent_plist(Path::new("/Applications/Clocky.app/Contents/MacOS/clocky"));
    assert!(text.starts_with("<?xml version=\"1.0\""), "{text}");
    assert!(text.contains("<string>app.clocky</string>"), "{text}");
    assert!(
        text.contains("<string>/Applications/Clocky.app/Contents/MacOS/clocky</string>"),
        "{text}"
    );
    assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"), "{text}");
    let odd = launch_agent_plist(Path::new("/tmp/R&D <new>/c'l\"y"));
    assert!(
        odd.contains("<string>/tmp/R&amp;D &lt;new&gt;/c&apos;l&quot;y</string>"),
        "{odd}"
    );
}

#[test]
fn run_value_is_the_quoted_path() {
    assert_eq!(
        run_value(Path::new(r"C:\Program Files\Clocky\clocky.exe")),
        r#""C:\Program Files\Clocky\clocky.exe""#
    );
}

#[test]
fn file_entry_writes_and_removes_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let autostart = dir.path().join("autostart");
    let entry = FileEntry::desktop(&autostart);
    assert!(!entry.is_enabled().unwrap());
    // Disabling with no entry (and no directory) is fine.
    entry.disable().unwrap();
    entry.enable(Path::new("/usr/bin/clocky")).unwrap();
    assert!(entry.is_enabled().unwrap());
    let written = fs::read_to_string(autostart.join("clocky.desktop")).unwrap();
    assert_eq!(written, desktop_entry(Path::new("/usr/bin/clocky")));
    // Enabling again replaces the entry (the exe may have moved).
    entry.enable(Path::new("/opt/clocky")).unwrap();
    let written = fs::read_to_string(autostart.join("clocky.desktop")).unwrap();
    assert!(written.contains("Exec=\"/opt/clocky\""));
    entry.disable().unwrap();
    assert!(!entry.is_enabled().unwrap());
}

#[test]
fn launch_agent_file_is_named_after_its_label() {
    let dir = tempfile::tempdir().unwrap();
    let entry = FileEntry::launch_agent(dir.path());
    entry.enable(Path::new("/x/clocky")).unwrap();
    assert!(dir.path().join("app.clocky.plist").exists());
    entry.disable().unwrap();
    assert!(!dir.path().join("app.clocky.plist").exists());
}

/// The registry code against a throwaway key (`HKCU\Software\ClockyTest-…`), never the real
/// Run key. The guard deletes the key even when an assertion fails.
#[cfg(windows)]
mod registry {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    use super::*;

    struct Throwaway(String);

    impl Drop for Throwaway {
        fn drop(&mut self) {
            RegKey::predef(HKEY_CURRENT_USER)
                .delete_subkey_all(&self.0)
                .unwrap_or_default();
        }
    }

    #[test]
    fn run_value_is_written_read_and_removed() {
        let key = Throwaway(format!(r"Software\ClockyTest-{}", std::process::id()));
        let reg = Registry::new(&format!(r"{}\Run", key.0));
        assert!(!reg.is_enabled().unwrap());
        reg.disable().unwrap();
        let exe = Path::new(r"C:\Program Files\Clocky\clocky.exe");
        reg.enable(exe).unwrap();
        assert!(reg.is_enabled().unwrap());
        let stored: String = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(format!(r"{}\Run", key.0))
            .unwrap()
            .get_value("Clocky")
            .unwrap();
        assert_eq!(stored, run_value(exe));
        reg.disable().unwrap();
        assert!(!reg.is_enabled().unwrap());
    }

    #[test]
    fn the_real_key_is_the_run_key() {
        assert_eq!(
            super::super::registry::RUN_KEY,
            r"Software\Microsoft\Windows\CurrentVersion\Run"
        );
    }
}
