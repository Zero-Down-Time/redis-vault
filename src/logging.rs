//! Logging configuration and initialization
//!
//! This module handles the initialization of the tracing/logging subsystem.
//! It supports both text and JSON log formats and respects environment variables
//! for controlling log levels.

pub fn init_logging(level: &str, format: &str) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new("warn").add_directive(
            format!("redis_vault={}", level)
                .parse()
                .unwrap(),
        )
    });

    match format {
        "json" => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .json()
                .flatten_event(true)
                .without_time()
                .with_target(false)
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(false)
                .init();
        }
    }
}
