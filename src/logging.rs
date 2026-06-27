use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{reload, EnvFilter, Registry};

use crate::error::FusionError;

pub(crate) fn parse_level(level: &str) -> Result<EnvFilter, FusionError> {
    match level {
        "debug" | "info" | "error" => Ok(EnvFilter::new(level)),
        other => Err(FusionError::InvalidRequest(format!("bad log level '{other}'"))),
    }
}

pub struct LogHandle {
    reload: reload::Handle<EnvFilter, Registry>,
}

impl LogHandle {
    pub fn set_level(&self, level: &str) -> Result<(), FusionError> {
        let filter = parse_level(level)?;
        self.reload
            .modify(|f| *f = filter)
            .map_err(|e| FusionError::Internal(format!("log reload: {e}")))
    }
}

pub fn init(level: &str, log_file: Option<&str>, to_stdout: bool) -> LogHandle {
    let filter = parse_level(level).unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, reload) = reload::Layer::new(filter);
    let registry = Registry::default().with(filter_layer);
    let stdout_layer =
        to_stdout.then(|| tracing_subscriber::fmt::layer().with_writer(std::io::stdout));
    let file_layer = log_file.and_then(|path| {
        let p = std::path::Path::new(path);
        let dir = p
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let name = p.file_name()?.to_str()?.to_string();
        let appender = tracing_appender::rolling::never(dir, name);
        Some(
            tracing_subscriber::fmt::layer()
                .with_writer(appender)
                .with_ansi(false),
        )
    });
    // Use try_init so multiple calls in tests don't panic (only the first call initializes the global subscriber)
    let _ = registry.with(stdout_layer).with(file_layer).try_init();
    LogHandle { reload }
}

/// Print the admin token directly to stdout exactly once, never through tracing (design §3/§9).
pub fn print_admin_token_once(token: &str) {
    println!("\n=== LocalFusion admin token (save it, shown only once) ===\n{token}\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_filter_parses() {
        assert!(parse_level("debug").is_ok());
        assert!(parse_level("info").is_ok());
        assert!(parse_level("error").is_ok());
        assert!(parse_level("bogus").is_err());
    }
}
