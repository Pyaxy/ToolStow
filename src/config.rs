//! CLI argument parsing and runtime configuration for ToolStow.

use std::path::PathBuf;

use clap::Parser;

/// Default bind host (loopback only for safe personal self-hosting).
pub const DEFAULT_HOST: &str = "127.0.0.1";
/// Default HTTP listen port.
pub const DEFAULT_PORT: u16 = 8080;
/// Default directory storing tool folders.
pub const DEFAULT_TOOLS_DIR: &str = "./tools";
/// Default maximum `.zip` upload size in MiB.
pub const DEFAULT_MAX_UPLOAD_MB: usize = 50;

/// Runtime configuration parsed from command-line flags.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(
    name = "toolstow",
    version,
    about = "Single-binary self-hosted personal micro-tools host"
)]
pub struct Config {
    /// Network interface / IP address to bind the HTTP server to.
    #[arg(long, default_value = DEFAULT_HOST)]
    pub host: String,

    /// TCP port to listen on.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Root directory containing installed tool subdirectories.
    #[arg(long, default_value = DEFAULT_TOOLS_DIR)]
    pub tools_dir: PathBuf,

    /// Maximum allowed `.zip` upload payload size in MiB.
    #[arg(long, default_value_t = DEFAULT_MAX_UPLOAD_MB)]
    pub max_upload_mb: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            tools_dir: PathBuf::from(DEFAULT_TOOLS_DIR),
            max_upload_mb: DEFAULT_MAX_UPLOAD_MB,
        }
    }
}

impl Config {
    /// Returns the maximum upload payload size in bytes (`max_upload_mb * 1024 * 1024`),
    /// saturating on overflow.
    pub fn max_upload_bytes(&self) -> usize {
        self.max_upload_mb.saturating_mul(1024 * 1024)
    }

    /// Returns the formatted `host:port` socket bind string.
    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let cfg = Config::try_parse_from(["toolstow"]).expect("should parse defaults");
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.tools_dir, PathBuf::from("./tools"));
        assert_eq!(cfg.max_upload_mb, 50);
        assert_eq!(cfg.max_upload_bytes(), 50 * 1024 * 1024);
        assert_eq!(cfg.listen_addr(), "127.0.0.1:8080");
    }

    #[test]
    fn parses_custom_flags() {
        let cfg = Config::try_parse_from([
            "toolstow",
            "--host",
            "0.0.0.0",
            "--port",
            "9090",
            "--tools-dir",
            "/srv/toolstow/tools",
            "--max-upload-mb",
            "128",
        ])
        .expect("should parse custom flags");

        assert_eq!(cfg.host, "0.0.0.0");
        assert_eq!(cfg.port, 9090);
        assert_eq!(cfg.tools_dir, PathBuf::from("/srv/toolstow/tools"));
        assert_eq!(cfg.max_upload_mb, 128);
        assert_eq!(cfg.max_upload_bytes(), 128 * 1024 * 1024);
        assert_eq!(cfg.listen_addr(), "0.0.0.0:9090");
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
