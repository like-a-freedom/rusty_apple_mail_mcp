use std::path::{Path, PathBuf};

pub fn envelope_db_path(mail_directory: &Path, mail_version: &str) -> PathBuf {
    mail_directory
        .join(mail_version)
        .join("MailData")
        .join("Envelope Index")
}

pub fn default_mail_directory() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library/Mail")
}

pub fn normalize_mail_directory(path: PathBuf) -> PathBuf {
    expand_mail_directory(&path.to_string_lossy())
}

pub fn expand_mail_directory(raw: &str) -> PathBuf {
    if raw == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }

    if let Some(stripped) = raw.strip_prefix("~/")
        && let Some(home_dir) = dirs::home_dir()
    {
        return home_dir.join(stripped);
    }

    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[test]
    fn expand_mail_directory_expands_tilde_prefix() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let expected = dirs::home_dir().expect("home dir").join("Library/Mail");

        assert_eq!(expand_mail_directory("~/Library/Mail"), expected);
    }
}
