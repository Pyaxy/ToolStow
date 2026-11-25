//! CLI argument parsing, `.env` loading, and runtime configuration for ToolStow.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use tracing::warn;

/// Default bind host (loopback only for safe personal self-hosting).
pub const DEFAULT_HOST: &str = "127.0.0.1";
/// Default HTTP listen port.
pub const DEFAULT_PORT: u16 = 8080;
/// Default root data directory storing `tools/`, `theme/`, and `settings.json`.
pub const DEFAULT_DATA_DIR: &str = "./data";
/// Default directory storing tool folders (derived from `DEFAULT_DATA_DIR`).
pub const DEFAULT_TOOLS_DIR: &str = "./data/tools";
/// Default directory storing optional custom public frontend theme overrides.
pub const DEFAULT_THEME_DIR: &str = "./data/theme";
/// Default directory storing installed theme packages (derived from `DEFAULT_DATA_DIR`).
pub const DEFAULT_THEMES_DIR: &str = "./data/themes";
/// Default maximum `.zip` upload size in MiB.
pub const DEFAULT_MAX_UPLOAD_MB: usize = 50;
/// Default setting for exposing draft tools to public visitors.
pub const DEFAULT_INCLUDE_DRAFTS: bool = false;
/// Default setting for forcing the `Secure` attribute on admin session cookies.
pub const DEFAULT_COOKIE_SECURE: bool = false;

/// Internal CLI argument definition supporting `CLI > std::env > .env > default` resolution.
#[derive(Debug, Parser)]
#[command(
    name = "toolstow",
    version,
    about = "Single-binary self-hosted personal micro-tools host"
)]
struct CliArgs {
    /// Network interface / IP address to bind the HTTP server to.
    #[arg(long)]
    host: Option<String>,

    /// TCP port to listen on.
    #[arg(long)]
    port: Option<u16>,

    /// Root data directory containing `tools/`, `theme/`, and `settings.json`.
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Override root directory containing installed tool subdirectories (defaults to `<data_dir>/tools`).
    #[arg(long)]
    tools_dir: Option<PathBuf>,

    /// Override directory containing custom static public theme overrides (defaults to `<data_dir>/theme`).
    #[arg(long)]
    theme_dir: Option<PathBuf>,

    /// Override root directory containing installed theme subdirectories (defaults to `<data_dir>/themes`).
    #[arg(long)]
    themes_dir: Option<PathBuf>,

    /// Maximum allowed `.zip` upload payload size in MiB.
    #[arg(long)]
    max_upload_mb: Option<usize>,

    /// Include `draft: true` tools in public listings and anonymous `/t/{id}/*` requests (useful for local preview).
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    include_drafts: Option<bool>,

    /// Force the `Secure` attribute on admin session cookies even behind plain HTTP reverse proxies.
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    cookie_secure: Option<bool>,
}

/// Runtime configuration resolved from CLI flags, process environment variables,
/// an optional local `.env` file, and built-in defaults.
#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    /// Network interface / IP address to bind the HTTP server to.
    pub host: String,
    /// TCP port to listen on.
    pub port: u16,
    /// Root data directory containing `tools/`, `theme/`, and `settings.json`.
    pub data_dir: PathBuf,
    /// Root directory containing installed tool subdirectories.
    pub tools_dir: PathBuf,
    /// Directory containing optional custom static public frontend overrides.
    pub theme_dir: PathBuf,
    /// Root directory containing installed theme subdirectories.
    pub themes_dir: PathBuf,
    /// Maximum allowed `.zip` upload payload size in MiB.
    pub max_upload_mb: usize,
    /// Whether draft tools are exposed publicly (intended for local development preview).
    pub include_drafts: bool,
    /// Whether admin session cookies always include the `Secure` attribute.
    pub cookie_secure: bool,
    /// Optional human admin login key (`TOOLSTOW_ADMIN_KEY`), trimmed (`None` if unset/empty).
    pub admin_key: Option<String>,
    /// Optional machine API Bearer key (`TOOLSTOW_API_KEY`), trimmed (`None` if unset/empty).
    pub api_key: Option<String>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("data_dir", &self.data_dir)
            .field("tools_dir", &self.tools_dir)
            .field("theme_dir", &self.theme_dir)
            .field("themes_dir", &self.themes_dir)
            .field("max_upload_mb", &self.max_upload_mb)
            .field("include_drafts", &self.include_drafts)
            .field("cookie_secure", &self.cookie_secure)
            .field(
                "admin_key",
                &self.admin_key.as_ref().map(|_| "[REDACTED]"),
            )
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl Default for Config {
    fn default() -> Self {
        let data_dir = PathBuf::from(DEFAULT_DATA_DIR);
        let tools_dir = data_dir.join("tools");
        let theme_dir = data_dir.join("theme");
        let themes_dir = data_dir.join("themes");
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            data_dir,
            tools_dir,
            theme_dir,
            themes_dir,
            max_upload_mb: DEFAULT_MAX_UPLOAD_MB,
            include_drafts: DEFAULT_INCLUDE_DRAFTS,
            cookie_secure: DEFAULT_COOKIE_SECURE,
            admin_key: None,
            api_key: None,
        }
    }
}

/// Parses `.env` formatted content into a `HashMap<String, String>` without touching
/// global process environment state (zero `unsafe`).
///
/// Rules:
/// - Ignores blank lines and `#` comment lines.
/// - Supports optional leading `export ` prefix.
/// - Splits on the first `=`; trims key and outer whitespace around value.
/// - Strips optional matching single (`'...'`) or double (`"..."`) quotes around values.
/// - Strips trailing inline `# ...` comments on unquoted values when preceded by whitespace.
pub fn parse_dotenv_str(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line
            .strip_prefix("export ")
            .or_else(|| line.strip_prefix("export\t"))
            .map(str::trim_start)
            .unwrap_or(line);

        let Some((raw_key, raw_val)) = line.split_once('=') else {
            continue;
        };

        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }

        let val_trimmed = raw_val.trim();
        let parsed_val = if val_trimmed.len() >= 2
            && ((val_trimmed.starts_with('"') && val_trimmed.ends_with('"'))
                || (val_trimmed.starts_with('\'') && val_trimmed.ends_with('\'')))
        {
            val_trimmed[1..val_trimmed.len() - 1].to_string()
        } else {
            strip_inline_comment(val_trimmed).trim().to_string()
        };

        map.insert(key.to_string(), parsed_val);
    }

    map
}

/// Strips a trailing `# ...` comment from an unquoted `.env` value when preceded by whitespace.
fn strip_inline_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return &value[..i];
        }
    }
    value
}

/// Loads key-value pairs from a `.env` file if it exists, returning an empty map if missing.
///
/// Never mutates process environment variables (zero `unsafe`).
pub fn load_dotenv(path: &Path) -> HashMap<String, String> {
    match fs::read_to_string(path) {
        Ok(content) => parse_dotenv_str(&content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(err) => {
            warn!(
                path = %path.display(),
                error = %err,
                "failed to read .env file; continuing without it"
            );
            HashMap::new()
        }
    }
}

impl Config {
    /// Parses CLI arguments from `std::env::args_os()`, loads `.env` from the current
    /// working directory if present, and resolves configuration with precedence:
    /// `CLI flags > process env vars > .env file > built-in defaults`.
    pub fn from_cli_and_env() -> Self {
        let dotenv = load_dotenv(Path::new(".env"));
        match Self::try_from_args_and_env(
            std::env::args_os(),
            |key| std::env::var(key).ok(),
            &dotenv,
        ) {
            Ok(cfg) => cfg,
            Err(err) => err.exit(),
        }
    }

    /// Resolves a [`Config`] from explicit CLI arguments, an environment lookup closure,
    /// and a parsed `.env` map using precedence `CLI > env_get > dotenv > defaults`.
    pub fn try_from_args_and_env<I, T, F>(
        args: I,
        env_get: F,
        dotenv: &HashMap<String, String>,
    ) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
        F: Fn(&str) -> Option<String>,
    {
        let cli = CliArgs::try_parse_from(args)?;

        let lookup_raw = |key: &str| -> Option<String> {
            env_get(key).or_else(|| dotenv.get(key).cloned())
        };

        let lookup_non_empty = |key: &str| -> Option<String> {
            lookup_raw(key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };

        let host = cli
            .host
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .or_else(|| lookup_non_empty("TOOLSTOW_HOST"))
            .unwrap_or_else(|| DEFAULT_HOST.to_string());

        let port = cli
            .port
            .or_else(|| {
                lookup_non_empty("TOOLSTOW_PORT").and_then(|raw| match raw.parse::<u16>() {
                    Ok(p) => Some(p),
                    Err(err) => {
                        warn!(value = %raw, error = %err, "invalid TOOLSTOW_PORT; using default");
                        None
                    }
                })
            })
            .unwrap_or(DEFAULT_PORT);

        let data_dir = cli
            .data_dir
            .or_else(|| lookup_non_empty("TOOLSTOW_DATA_DIR").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));

        let tools_dir = cli
            .tools_dir
            .or_else(|| lookup_non_empty("TOOLSTOW_TOOLS_DIR").map(PathBuf::from))
            .unwrap_or_else(|| data_dir.join("tools"));

        let theme_dir = cli
            .theme_dir
            .or_else(|| lookup_non_empty("TOOLSTOW_THEME_DIR").map(PathBuf::from))
            .unwrap_or_else(|| data_dir.join("theme"));

        let themes_dir = cli
            .themes_dir
            .or_else(|| lookup_non_empty("TOOLSTOW_THEMES_DIR").map(PathBuf::from))
            .unwrap_or_else(|| data_dir.join("themes"));

        let max_upload_mb = cli
            .max_upload_mb
            .or_else(|| {
                lookup_non_empty("TOOLSTOW_MAX_UPLOAD_MB").and_then(|raw| {
                    match raw.parse::<usize>() {
                        Ok(mb) => Some(mb),
                        Err(err) => {
                            warn!(
                                value = %raw,
                                error = %err,
                                "invalid TOOLSTOW_MAX_UPLOAD_MB; using default"
                            );
                            None
                        }
                    }
                })
            })
            .unwrap_or(DEFAULT_MAX_UPLOAD_MB);

        let include_drafts = cli
            .include_drafts
            .or_else(|| {
                lookup_non_empty("TOOLSTOW_INCLUDE_DRAFTS")
                    .and_then(|raw| parse_env_bool("TOOLSTOW_INCLUDE_DRAFTS", &raw))
            })
            .unwrap_or(DEFAULT_INCLUDE_DRAFTS);

        let cookie_secure = cli
            .cookie_secure
            .or_else(|| {
                lookup_non_empty("TOOLSTOW_COOKIE_SECURE")
                    .and_then(|raw| parse_env_bool("TOOLSTOW_COOKIE_SECURE", &raw))
            })
            .unwrap_or(DEFAULT_COOKIE_SECURE);

        let admin_key = lookup_non_empty("TOOLSTOW_ADMIN_KEY");
        let api_key = lookup_non_empty("TOOLSTOW_API_KEY");

        Ok(Self {
            host,
            port,
            data_dir,
            tools_dir,
            theme_dir,
            themes_dir,
            max_upload_mb,
            include_drafts,
            cookie_secure,
            admin_key,
            api_key,
        })
    }

    /// Returns the maximum upload payload size in bytes (`max_upload_mb * 1024 * 1024`),
    /// saturating on overflow.
    pub fn max_upload_bytes(&self) -> usize {
        self.max_upload_mb.saturating_mul(1024 * 1024)
    }

    /// Returns the formatted `host:port` socket bind string.
    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Returns the path to the persisted site settings JSON file (`<data_dir>/settings.json`).
    pub fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }
}

/// Parses a boolean environment variable value (`1/true/yes/on` vs `0/false/no/off`).
fn parse_env_bool(name: &str, raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        other => {
            warn!(env = name, value = %other, "invalid boolean env value; using default");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn parses_defaults_and_derived_paths() {
        let cfg = Config::try_from_args_and_env(["toolstow"], no_env, &HashMap::new())
            .expect("should parse defaults");
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.data_dir, PathBuf::from("./data"));
        assert_eq!(cfg.tools_dir, PathBuf::from("./data/tools"));
        assert_eq!(cfg.theme_dir, PathBuf::from("./data/theme"));
        assert_eq!(cfg.themes_dir, PathBuf::from(DEFAULT_THEMES_DIR));
        assert_eq!(cfg.settings_path(), PathBuf::from("./data/settings.json"));
        assert_eq!(cfg.max_upload_mb, 50);
        assert_eq!(cfg.max_upload_bytes(), 50 * 1024 * 1024);
        assert_eq!(cfg.listen_addr(), "127.0.0.1:8080");
        assert!(!cfg.include_drafts);
        assert!(!cfg.cookie_secure);
        assert_eq!(cfg.admin_key, None);
        assert_eq!(cfg.api_key, None);
    }

    #[test]
    fn parses_dotenv_comments_quotes_and_inline_comments() {
        let content = "\u{feff}\
# leading comment
   # indented comment

TOOLSTOW_HOST=0.0.0.0 # inline comment
TOOLSTOW_PORT=\"9090\"
export TOOLSTOW_DATA_DIR='/srv/toolstow'
TOOLSTOW_ADMIN_KEY=\"secret#with#hash\"
TOOLSTOW_API_KEY='  single-quoted-key  '
EMPTY_KEY=
MALFORMED_LINE_WITHOUT_EQUALS
=missing_key
";
        let map = parse_dotenv_str(content);
        assert_eq!(map.get("TOOLSTOW_HOST").map(String::as_str), Some("0.0.0.0"));
        assert_eq!(map.get("TOOLSTOW_PORT").map(String::as_str), Some("9090"));
        assert_eq!(
            map.get("TOOLSTOW_DATA_DIR").map(String::as_str),
            Some("/srv/toolstow")
        );
        assert_eq!(
            map.get("TOOLSTOW_ADMIN_KEY").map(String::as_str),
            Some("secret#with#hash")
        );
        assert_eq!(
            map.get("TOOLSTOW_API_KEY").map(String::as_str),
            Some("  single-quoted-key  ")
        );
        assert_eq!(map.get("EMPTY_KEY").map(String::as_str), Some(""));
        assert!(!map.contains_key("MALFORMED_LINE_WITHOUT_EQUALS"));
    }

    #[test]
    fn load_dotenv_reads_file_and_returns_empty_when_missing() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join(".env");
        assert!(load_dotenv(&missing).is_empty());

        fs::write(&missing, "TOOLSTOW_PORT=7777\nTOOLSTOW_INCLUDE_DRAFTS=true\n").unwrap();
        let loaded = load_dotenv(&missing);
        assert_eq!(loaded.get("TOOLSTOW_PORT").map(String::as_str), Some("7777"));
        assert_eq!(
            loaded.get("TOOLSTOW_INCLUDE_DRAFTS").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn resolves_cli_over_env_over_dotenv_and_derives_subdirectories() {
        let dotenv = parse_dotenv_str(
            r#"
TOOLSTOW_HOST=10.0.0.1
TOOLSTOW_PORT=7000
TOOLSTOW_DATA_DIR=/dotenv/data
TOOLSTOW_MAX_UPLOAD_MB=25
TOOLSTOW_INCLUDE_DRAFTS=true
TOOLSTOW_COOKIE_SECURE=true
TOOLSTOW_ADMIN_KEY=dotenv-admin-key-12345
TOOLSTOW_API_KEY=dotenv-api-key-12345
"#,
        );

        let mut proc_env = HashMap::new();
        proc_env.insert("TOOLSTOW_HOST", "10.0.0.2");
        proc_env.insert("TOOLSTOW_PORT", "8000");
        proc_env.insert("TOOLSTOW_DATA_DIR", "/env/data");
        proc_env.insert("TOOLSTOW_ADMIN_KEY", "  env-admin-key-12345  ");
        // Empty env var overrides dotenv and resolves to None!
        proc_env.insert("TOOLSTOW_API_KEY", "   ");

        let cfg = Config::try_from_args_and_env(
            [
                "toolstow",
                "--port",
                "9090",
                "--tools-dir",
                "/custom/tools",
                "--include-drafts=false",
            ],
            |k| proc_env.get(k).map(|v| (*v).to_string()),
            &dotenv,
        )
        .expect("should resolve config");

        // CLI > proc_env > dotenv
        assert_eq!(cfg.host, "10.0.0.2");
        assert_eq!(cfg.port, 9090);
        assert_eq!(cfg.data_dir, PathBuf::from("/env/data"));
        assert_eq!(cfg.tools_dir, PathBuf::from("/custom/tools"));
        assert_eq!(cfg.theme_dir, PathBuf::from("/env/data/theme"));
        assert_eq!(cfg.themes_dir, PathBuf::from("/env/data/themes"));
        assert_eq!(cfg.settings_path(), PathBuf::from("/env/data/settings.json"));
        assert_eq!(cfg.max_upload_mb, 25);
        assert!(!cfg.include_drafts);
        assert!(cfg.cookie_secure);
        assert_eq!(cfg.admin_key.as_deref(), Some("env-admin-key-12345"));
        assert_eq!(cfg.api_key, None);
    }

    #[test]
    fn derives_tools_and_theme_from_cli_data_dir_and_handles_bare_bool_flags() {
        let cfg = Config::try_from_args_and_env(
            [
                "toolstow",
                "--data-dir",
                "/var/lib/toolstow",
                "--include-drafts",
                "--cookie-secure",
            ],
            no_env,
            &HashMap::new(),
        )
        .expect("should parse bare flags");

        assert_eq!(cfg.data_dir, PathBuf::from("/var/lib/toolstow"));
        assert_eq!(cfg.tools_dir, PathBuf::from("/var/lib/toolstow/tools"));
        assert_eq!(cfg.theme_dir, PathBuf::from("/var/lib/toolstow/theme"));
        assert_eq!(cfg.themes_dir, PathBuf::from("/var/lib/toolstow/themes"));
        assert_eq!(
            cfg.settings_path(),
            PathBuf::from("/var/lib/toolstow/settings.json")
        );
        assert!(cfg.include_drafts);
        assert!(cfg.cookie_secure);
    }

    #[test]
    fn resolves_themes_dir_cli_over_env_over_dotenv() {
        let dotenv = parse_dotenv_str("TOOLSTOW_THEMES_DIR=/dotenv/themes\n");
        let cfg_dotenv =
            Config::try_from_args_and_env(["toolstow"], no_env, &dotenv).expect("dotenv themes");
        assert_eq!(cfg_dotenv.themes_dir, PathBuf::from("/dotenv/themes"));

        let mut proc_env = HashMap::new();
        proc_env.insert("TOOLSTOW_THEMES_DIR", "/env/themes");
        let cfg_env = Config::try_from_args_and_env(
            ["toolstow"],
            |k| proc_env.get(k).map(|v| (*v).to_string()),
            &dotenv,
        )
        .expect("env themes");
        assert_eq!(cfg_env.themes_dir, PathBuf::from("/env/themes"));

        let cfg_cli = Config::try_from_args_and_env(
            ["toolstow", "--themes-dir", "/cli/themes"],
            |k| proc_env.get(k).map(|v| (*v).to_string()),
            &dotenv,
        )
        .expect("cli themes");
        assert_eq!(cfg_cli.themes_dir, PathBuf::from("/cli/themes"));
    }

    #[test]
    fn empty_or_whitespace_keys_resolve_to_none_and_debug_redacts_secrets() {
        let dotenv = parse_dotenv_str(
            r#"
TOOLSTOW_ADMIN_KEY=""
TOOLSTOW_API_KEY="   "
"#,
        );
        let cfg = Config::try_from_args_and_env(["toolstow"], no_env, &dotenv).unwrap();
        assert_eq!(cfg.admin_key, None);
        assert_eq!(cfg.api_key, None);

        let secret_cfg = Config {
            admin_key: Some("super-secret-admin-key".to_string()),
            api_key: Some("super-secret-api-key".to_string()),
            ..Config::default()
        };
        let debug_str = format!("{secret_cfg:?}");
        assert!(!debug_str.contains("super-secret-admin-key"));
        assert!(!debug_str.contains("super-secret-api-key"));
        assert!(debug_str.contains("[REDACTED]"));
    }

    #[test]
    fn max_upload_bytes_saturates_on_overflow() {
        let cfg = Config {
            max_upload_mb: usize::MAX,
            ..Config::default()
        };
        assert_eq!(cfg.max_upload_bytes(), usize::MAX);
    }
}
