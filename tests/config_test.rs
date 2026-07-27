use std::path::Path;

use once_cell::sync::Lazy;
use std::sync::Mutex;
use tempfile::TempDir;

use rusty_apple_mail_mcp::config::{MailConfigBuilder, MailConfigOverrides};

static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn create_db_for_version(base: &Path, version: &str) {
    let db_dir = base.join(version).join("MailData");
    std::fs::create_dir_all(&db_dir).expect("mail data dir");
    std::fs::write(db_dir.join("Envelope Index"), b"sqlite placeholder").expect("db placeholder");
}

#[test]
fn builder_defaults_when_no_env_or_config() {
    let temp_dir = TempDir::new().expect("temp dir");
    let mail_directory = temp_dir.path().to_path_buf();
    create_db_for_version(&mail_directory, "V10");

    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::remove_var("APPLE_MAIL_DIR");
        std::env::remove_var("APPLE_MAIL_VERSION");
        std::env::remove_var("APPLE_MAIL_ACCOUNT");
        std::env::remove_var("APPLE_MAIL_LOG_LEVEL");
        std::env::set_var("HOME", temp_dir.path());
    }

    let config = MailConfigBuilder::new()
        .mail_directory(mail_directory.clone())
        .mail_version("V10".to_string())
        .build()
        .expect("config should build");

    assert_eq!(config.mail_directory, mail_directory);
    assert_eq!(config.mail_version, "V10");
    assert!(config.allowed_account_ids().is_none());
    assert!(config.log_level.is_none());
}

#[test]
fn builder_env_vars_override_defaults() {
    let temp_dir = TempDir::new().expect("temp dir");
    let mail_directory = temp_dir.path().to_path_buf();
    create_db_for_version(&mail_directory, "V9");

    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::set_var("APPLE_MAIL_DIR", &mail_directory);
        std::env::set_var("APPLE_MAIL_VERSION", "V9");
        std::env::set_var("APPLE_MAIL_LOG_LEVEL", "debug");
    }

    let config = MailConfigBuilder::new()
        .build()
        .expect("config should build");

    assert_eq!(config.mail_directory, mail_directory);
    assert_eq!(config.mail_version, "V9");
    assert_eq!(config.log_level.as_deref(), Some("debug"));

    unsafe {
        std::env::remove_var("APPLE_MAIL_DIR");
        std::env::remove_var("APPLE_MAIL_VERSION");
        std::env::remove_var("APPLE_MAIL_LOG_LEVEL");
    }
}

#[test]
fn builder_yaml_config_provides_fallback() {
    let temp_dir = TempDir::new().expect("temp dir");
    let mail_directory = temp_dir.path().to_path_buf();
    create_db_for_version(&mail_directory, "V8");

    let config_dir = temp_dir.path().join(".config").join("rusty_apple_mail_mcp");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(
        config_dir.join("config.yaml"),
        "apple_mail_version: \"V8\"\nlog_level: \"info\"\n",
    )
    .expect("write config");

    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::remove_var("APPLE_MAIL_DIR");
        std::env::remove_var("APPLE_MAIL_VERSION");
        std::env::remove_var("APPLE_MAIL_LOG_LEVEL");
        std::env::set_var("HOME", temp_dir.path());
    }

    let config = MailConfigBuilder::new()
        .mail_directory(mail_directory.clone())
        .build()
        .expect("config should build");

    assert_eq!(config.mail_directory, mail_directory);
    assert_eq!(config.mail_version, "V8");
    assert_eq!(config.log_level.as_deref(), Some("info"));
}

#[test]
fn builder_cli_overrides_env_overrides_yaml() {
    let temp_dir = TempDir::new().expect("temp dir");
    let mail_directory = temp_dir.path().to_path_buf();
    create_db_for_version(&mail_directory, "V9");

    let config_dir = temp_dir.path().join(".config").join("rusty_apple_mail_mcp");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(
        config_dir.join("config.yaml"),
        "apple_mail_version: \"V7\"\n",
    )
    .expect("write config");

    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::set_var("APPLE_MAIL_DIR", &mail_directory);
        std::env::remove_var("APPLE_MAIL_VERSION");
        std::env::remove_var("APPLE_MAIL_ACCOUNT");
        std::env::set_var("HOME", temp_dir.path());
    }

    let config = MailConfigBuilder::from_overrides(MailConfigOverrides {
        mail_directory: None,
        mail_version: Some("V9".to_string()),
        account: None,
    })
    .expect("config should build");

    // CLI wins over env and yaml
    assert_eq!(config.mail_version, "V9");

    unsafe {
        std::env::remove_var("APPLE_MAIL_DIR");
    }
}

#[test]
fn builder_missing_database_returns_error() {
    let temp_dir = TempDir::new().expect("temp dir");
    let mail_directory = temp_dir.path().to_path_buf();

    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::remove_var("APPLE_MAIL_DIR");
        std::env::remove_var("APPLE_MAIL_VERSION");
        std::env::remove_var("APPLE_MAIL_ACCOUNT");
    }

    let result = MailConfigBuilder::new()
        .mail_directory(mail_directory)
        .mail_version("V10".to_string())
        .build();

    assert!(result.is_err());
}

#[test]
fn builder_log_level_from_yaml() {
    let temp_dir = TempDir::new().expect("temp dir");
    let mail_directory = temp_dir.path().to_path_buf();
    create_db_for_version(&mail_directory, "V10");

    let config_dir = temp_dir.path().join(".config").join("rusty_apple_mail_mcp");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(config_dir.join("config.yaml"), "log_level: \"debug\"\n").expect("write config");

    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::remove_var("APPLE_MAIL_DIR");
        std::env::remove_var("APPLE_MAIL_VERSION");
        std::env::remove_var("APPLE_MAIL_ACCOUNT");
        std::env::remove_var("APPLE_MAIL_LOG_LEVEL");
        std::env::set_var("HOME", temp_dir.path());
    }

    let config = MailConfigBuilder::new()
        .mail_directory(mail_directory)
        .build()
        .expect("config should build");

    assert_eq!(config.log_level.as_deref(), Some("debug"));
}

#[test]
fn builder_env_log_level_overrides_yaml() {
    let temp_dir = TempDir::new().expect("temp dir");
    let mail_directory = temp_dir.path().to_path_buf();
    create_db_for_version(&mail_directory, "V10");

    let config_dir = temp_dir.path().join(".config").join("rusty_apple_mail_mcp");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(config_dir.join("config.yaml"), "log_level: \"debug\"\n").expect("write config");

    let _guard = ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::remove_var("APPLE_MAIL_DIR");
        std::env::remove_var("APPLE_MAIL_VERSION");
        std::env::remove_var("APPLE_MAIL_ACCOUNT");
        std::env::set_var("APPLE_MAIL_LOG_LEVEL", "trace");
        std::env::set_var("HOME", temp_dir.path());
    }

    let config = MailConfigBuilder::new()
        .mail_directory(mail_directory)
        .build()
        .expect("config should build");

    // Env var wins over yaml
    assert_eq!(config.log_level.as_deref(), Some("trace"));

    unsafe {
        std::env::remove_var("APPLE_MAIL_LOG_LEVEL");
    }
}
