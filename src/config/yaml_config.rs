use std::path::PathBuf;

const CONFIG_FILENAME: &str = "config.yaml";

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, Default)]
pub struct YamlConfig {
    pub apple_mail_dir: Option<String>,
    pub apple_mail_version: Option<String>,
    pub apple_mail_account: Option<String>,
    pub log_level: Option<String>,
}

impl YamlConfig {
    pub fn load() -> Option<Self> {
        Self::load_from_path().or_else(Self::load_from_home)
    }

    fn load_from_path() -> Option<Self> {
        let path = Self::config_path()?;
        let content = std::fs::read_to_string(&path).ok()?;
        serde_yaml::from_str(&content).ok()
    }

    fn load_from_home() -> Option<Self> {
        let path = Self::config_path_home()?;
        let content = std::fs::read_to_string(&path).ok()?;
        serde_yaml::from_str(&content).ok()
    }

    fn config_path() -> Option<PathBuf> {
        let exe_dir = std::env::current_exe().ok()?;
        let dir = exe_dir.parent()?;
        Some(dir.join(CONFIG_FILENAME))
    }

    fn config_path_home() -> Option<PathBuf> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()?;
        Some(
            PathBuf::from(home)
                .join(".config")
                .join("rusty_apple_mail_mcp")
                .join(CONFIG_FILENAME),
        )
    }
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(stripped);
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn yaml_config_default_is_all_none() {
        let config = YamlConfig::default();
        assert!(config.apple_mail_dir.is_none());
        assert!(config.apple_mail_version.is_none());
        assert!(config.apple_mail_account.is_none());
        assert!(config.log_level.is_none());
    }

    #[test]
    fn yaml_config_loads_from_valid_yaml() {
        let yaml = r#"
apple_mail_dir: "/custom/mail"
apple_mail_version: "V9"
apple_mail_account: "Work Email"
log_level: "debug"
"#;
        let config: YamlConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert_eq!(config.apple_mail_dir.as_deref(), Some("/custom/mail"));
        assert_eq!(config.apple_mail_version.as_deref(), Some("V9"));
        assert_eq!(config.apple_mail_account.as_deref(), Some("Work Email"));
        assert_eq!(config.log_level.as_deref(), Some("debug"));
    }

    #[test]
    fn yaml_config_loads_partial_yaml() {
        let yaml = r#"
apple_mail_version: "V8"
"#;
        let config: YamlConfig = serde_yaml::from_str(yaml).expect("partial yaml");
        assert!(config.apple_mail_dir.is_none());
        assert_eq!(config.apple_mail_version.as_deref(), Some("V8"));
        assert!(config.apple_mail_account.is_none());
        assert!(config.log_level.is_none());
    }

    #[test]
    fn yaml_config_loads_empty_yaml() {
        let config: YamlConfig = serde_yaml::from_str("").expect("empty yaml");
        assert_eq!(config, YamlConfig::default());
    }

    #[test]
    fn expand_tilde_expands_prefix() {
        let home = dirs::home_dir().expect("home dir");
        assert_eq!(expand_tilde("~/Mail"), home.join("Mail"));
    }

    #[test]
    fn expand_tilde_no_prefix() {
        assert_eq!(
            expand_tilde("/absolute/path"),
            PathBuf::from("/absolute/path")
        );
    }

    #[test]
    fn yaml_config_loads_from_file_in_home() {
        let temp_dir = TempDir::new().expect("temp dir");
        let config_dir = temp_dir.path().join(".config").join("rusty_apple_mail_mcp");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        std::fs::write(
            config_dir.join(CONFIG_FILENAME),
            "apple_mail_version: \"V9\"\n",
        )
        .expect("write config");

        // Temporarily override HOME to point to our temp dir
        let original_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", temp_dir.path());
        }

        let config = YamlConfig::load().expect("should load from home config");
        assert_eq!(config.apple_mail_version.as_deref(), Some("V9"));

        // Restore HOME
        match original_home {
            Some(home) => unsafe { std::env::set_var("HOME", home) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }
}
