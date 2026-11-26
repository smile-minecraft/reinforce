use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::str::FromStr;

use reinforce_core::Error as CoreError;
use reinforce_core::Timestamp;

/// The single error type exposed to Swift and Kotlin.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("invalid {field}: {message}")]
    InvalidInput { field: String, message: String },
    #[error("not found: {message}")]
    NotFound { message: String },
    #[error("core error: {message}")]
    Core { message: String },
    #[error("io error: {message}")]
    Io { message: String },
    #[error("lock poisoned: {resource}")]
    LockPoisoned { resource: String },
    #[error("internal error: {message}")]
    Internal { message: String },
    #[error("panic while {operation}")]
    Panic { operation: String },
}

impl From<CoreError> for FfiError {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::InvalidValue(field, message) => Self::InvalidInput {
                field: field.to_owned(),
                message,
            },
            other => Self::Core {
                message: other.to_string(),
            },
        }
    }
}

impl From<io::Error> for FfiError {
    fn from(error: io::Error) -> Self {
        Self::Io {
            message: error.to_string(),
        }
    }
}

impl From<serde_json::Error> for FfiError {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidInput {
            field: "json".to_owned(),
            message: error.to_string(),
        }
    }
}

impl FfiError {
    pub(crate) fn invalid(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidInput {
            field: field.into(),
            message: message.into(),
        }
    }

    pub(crate) fn missing(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
        }
    }
}

pub(crate) fn guard<T>(
    operation: &str,
    body: impl FnOnce() -> Result<T, FfiError>,
) -> Result<T, FfiError> {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|panic| Err(panic_error(operation, panic)))
}

fn panic_error(operation: &str, panic: Box<dyn std::any::Any + Send>) -> FfiError {
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("unknown panic");
    FfiError::Panic {
        operation: format!("{operation}: {message}"),
    }
}

pub(crate) fn parse_id<T>(field: &str, value: &str) -> Result<T, FfiError>
where
    T: FromStr<Err = CoreError>,
{
    value.parse::<T>().map_err(|error| match error {
        CoreError::InvalidValue(_, message) => FfiError::invalid(field, message),
        other => FfiError::invalid(field, other.to_string()),
    })
}

pub(crate) fn parse_timestamp(field: &str, value: &str) -> Result<Timestamp, FfiError> {
    value.parse::<Timestamp>().map_err(|error| match error {
        CoreError::InvalidValue(_, message) => FfiError::invalid(field, message),
        other => FfiError::invalid(field, other.to_string()),
    })
}

pub(crate) fn usize_field(field: &str, value: u32) -> Result<usize, FfiError> {
    usize::try_from(value).map_err(|_| FfiError::invalid(field, "value is too large".to_owned()))
}

pub(crate) fn u32_field(field: &str, value: usize) -> Result<u32, FfiError> {
    u32::try_from(value).map_err(|_| FfiError::invalid(field, "value is too large".to_owned()))
}

pub(crate) fn finite(field: &str, value: f64) -> Result<f64, FfiError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(FfiError::invalid(field, "value must be finite".to_owned()))
    }
}

/// Adapter for `Mutex::lock` that turns poisoning into the exported error enum.
pub(crate) fn locked<'a, T>(
    resource: &str,
    mutex: &'a std::sync::Mutex<T>,
) -> Result<std::sync::MutexGuard<'a, T>, FfiError> {
    mutex.lock().map_err(|_| FfiError::LockPoisoned {
        resource: resource.to_owned(),
    })
}
