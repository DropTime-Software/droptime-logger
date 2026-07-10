//! `LoggerError` — the single error type every IPC command returns.
//!
//! Serializes to `{ code, message }` per CONTRACTS.md §2. Codes are the frozen
//! Phase 1 set (`source_not_found`, `session_active`, `no_such_roast`, `db`,
//! `io`) plus the v0.1.0 expansion set (CONTRACTS.md §7): `not_implemented`,
//! `port_busy`, `port_error`, `parse`, `invalid_args`, `preview_active`.

use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    SourceNotFound,
    SessionActive,
    NoSuchRoast,
    Db,
    Io,
    PortBusy,
    PortError,
    Parse,
    InvalidArgs,
    PreviewActive,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::SourceNotFound => "source_not_found",
            ErrorCode::SessionActive => "session_active",
            ErrorCode::NoSuchRoast => "no_such_roast",
            ErrorCode::Db => "db",
            ErrorCode::Io => "io",
            ErrorCode::PortBusy => "port_busy",
            ErrorCode::PortError => "port_error",
            ErrorCode::Parse => "parse",
            ErrorCode::InvalidArgs => "invalid_args",
            ErrorCode::PreviewActive => "preview_active",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoggerError {
    pub code: ErrorCode,
    pub message: String,
}

impl LoggerError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn db(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Db, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Io, message)
    }

    pub fn source_not_found(source_id: &str) -> Self {
        Self::new(
            ErrorCode::SourceNotFound,
            format!("unknown source: {source_id}"),
        )
    }

    pub fn session_active() -> Self {
        Self::new(
            ErrorCode::SessionActive,
            "a recording session is already active; only one session may record at a time",
        )
    }

    pub fn no_such_roast(roast_uuid: &str) -> Self {
        Self::new(
            ErrorCode::NoSuchRoast,
            format!("no such roast: {roast_uuid}"),
        )
    }

    pub fn port_busy(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::PortBusy, message)
    }

    #[allow(dead_code)]
    pub fn port_error(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::PortError, message)
    }

    #[allow(dead_code)]
    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Parse, message)
    }

    pub fn invalid_args(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgs, message)
    }

    #[allow(dead_code)]
    pub fn preview_active() -> Self {
        Self::new(
            ErrorCode::PreviewActive,
            "a port preview is already running; stop it before starting another",
        )
    }
}

impl fmt::Display for LoggerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for LoggerError {}

impl From<rusqlite::Error> for LoggerError {
    fn from(err: rusqlite::Error) -> Self {
        LoggerError::db(err.to_string())
    }
}

impl From<std::io::Error> for LoggerError {
    fn from(err: std::io::Error) -> Self {
        LoggerError::io(err.to_string())
    }
}
