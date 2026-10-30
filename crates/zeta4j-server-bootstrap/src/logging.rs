//! Logging initialization
//!
//! 서버 로깅 설정 초기화

use std::path::PathBuf;

use log::LevelFilter;
use log4rs::append::console::{ConsoleAppender, Target};
use log4rs::append::rolling_file::RollingFileAppender;
use log4rs::append::rolling_file::policy::compound::CompoundPolicy;
use log4rs::append::rolling_file::policy::compound::roll::fixed_window::FixedWindowRoller;
use log4rs::append::rolling_file::policy::compound::trigger::size::SizeTrigger;
use log4rs::config::{Appender, Config, Logger, Root};
use log4rs::encode::json::JsonEncoder;
use log4rs::encode::pattern::PatternEncoder;

use super::ValidatedStartupConfig;
use zeta4j_common::Edition;
use zeta4j_common::private_fs::{ensure_private_dir, prepare_private_file};

/// Initialize logging based on configuration
pub fn init_logging(
    edition: Edition,
    config: &ValidatedStartupConfig,
    log_config_path: Option<&PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let log_level = parse_log_level(&config.log_level);
    let use_json_format = config.log_format.to_lowercase() == "json";

    // 1. --log-config option specified
    if let Some(config_path) = log_config_path {
        if config_path.exists() {
            // log4rs::init_file already sets log level from config file
            log4rs::init_file(config_path, Default::default())?;
            return Ok(());
        } else {
            eprintln!(
                "Warning: Log config file not found: {}",
                config_path.display()
            );
            eprintln!("Using default logging configuration");
        }
    }

    // 2. Default config file path under the selected configuration directory.
    let default_config_path = config.config_dir.join("log4rs.toml");

    // Create config directory
    ensure_private_dir(&config.config_dir)?;

    // Use programmatic config if level/format is non-default
    let use_programmatic_config = config.log_level.to_lowercase() != "info" || use_json_format;

    // Use default config file (only if programmatic config not needed)
    if !use_programmatic_config && default_config_path.exists() {
        log4rs::init_file(&default_config_path, Default::default())?;
        return Ok(());
    }

    // 3. Programmatic configuration (fallback)
    init_programmatic_logging(edition, config, log_level, use_json_format)
}

/// Parse log level string to LevelFilter
fn parse_log_level(level: &str) -> LevelFilter {
    match level.to_lowercase().as_str() {
        "trace" => LevelFilter::Trace,
        "debug" => LevelFilter::Debug,
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Info,
    }
}

/// Initialize logging programmatically
fn init_programmatic_logging(
    edition: Edition,
    config: &ValidatedStartupConfig,
    log_level: LevelFilter,
    use_json_format: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let use_local_time = config.timezone.to_lowercase() == "local";

    // Ensure log directory exists
    ensure_private_dir(&config.log_dir)?;

    let log_file_path = config.log_dir.join(edition.log_file_name());
    prepare_private_file(&log_file_path)?;
    let archive_pattern = config.log_dir.join(format!(
        "{}.{{}}.log.gz",
        edition.log_file_name().trim_end_matches(".log")
    ));

    // Rolling file appender (configurable size and count)
    let rotation_size_bytes = config.log_rotation_size_mb * 1024 * 1024;
    let rotation_count = config.log_rotation_count;

    let roller = FixedWindowRoller::builder()
        .base(1)
        .build(&archive_pattern.to_string_lossy(), rotation_count)?;

    let trigger = SizeTrigger::new(rotation_size_bytes);
    let policy = CompoundPolicy::new(Box::new(trigger), Box::new(roller));

    // Build appenders based on format
    let (console, file) = if use_json_format {
        build_json_appenders(&log_file_path, policy)?
    } else {
        build_text_appenders(&log_file_path, policy, use_local_time)?
    };

    // Build config
    let log_config = Config::builder()
        .appender(Appender::builder().build("console", Box::new(console)))
        .appender(Appender::builder().build("file", Box::new(file)))
        // Quiet external libraries
        .logger(Logger::builder().build("hyper", LevelFilter::Warn))
        .logger(Logger::builder().build("tower_http", LevelFilter::Warn))
        .logger(Logger::builder().build("tokio", LevelFilter::Warn))
        .logger(Logger::builder().build("rustls", LevelFilter::Warn))
        .logger(Logger::builder().build("h2", LevelFilter::Warn))
        .build(
            Root::builder()
                .appender("console")
                .appender("file")
                .build(log_level),
        )?;

    log4rs::init_config(log_config)?;
    Ok(())
}

/// Build JSON format appenders
fn build_json_appenders(
    log_file_path: &std::path::Path,
    policy: CompoundPolicy,
) -> Result<(ConsoleAppender, RollingFileAppender), Box<dyn std::error::Error>> {
    let console = ConsoleAppender::builder()
        .target(Target::Stdout)
        .encoder(Box::new(JsonEncoder::new()))
        .build();

    let file = RollingFileAppender::builder()
        .encoder(Box::new(JsonEncoder::new()))
        .build(log_file_path, Box::new(policy))?;

    Ok((console, file))
}

/// Build text format appenders
fn build_text_appenders(
    log_file_path: &std::path::Path,
    policy: CompoundPolicy,
    use_local_time: bool,
) -> Result<(ConsoleAppender, RollingFileAppender), Box<dyn std::error::Error>> {
    let time_pattern = if use_local_time {
        "{d(%Y-%m-%d %H:%M:%S%.3f %z)}"
    } else {
        "{d(%Y-%m-%d %H:%M:%S%.3f)(utc)} UTC"
    };

    let console_pattern = format!("{} [{{h({{l}})}}] {{t}} - {{m}}{{n}}", time_pattern);
    let file_pattern = format!("{} [{{l}}] [{{T}}] {{t}} - {{m}}{{n}}", time_pattern);

    let console = ConsoleAppender::builder()
        .target(Target::Stdout)
        .encoder(Box::new(PatternEncoder::new(&console_pattern)))
        .build();

    let file = RollingFileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(&file_pattern)))
        .build(log_file_path, Box::new(policy))?;

    Ok((console, file))
}
