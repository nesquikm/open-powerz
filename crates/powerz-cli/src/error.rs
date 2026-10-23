//! [`CliError`]: a CLI failure and its process exit code.

use powerz_protocol::{pdm, session, transport};

/// A CLI failure, carrying its exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// Bad arguments or an ambiguous device choice (exit code 2).
    Usage(String),
    /// No device matches (exit code 3).
    NoDevice(String),
    /// The OS refused access to the device (exit code 4).
    Permission(String),
    /// Device or protocol failure (exit code 5).
    Device(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Usage(m)
            | CliError::NoDevice(m)
            | CliError::Permission(m)
            | CliError::Device(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for CliError {}

impl CliError {
    /// Process exit code: 2 usage, 3 no device, 4 permission, 5 device.
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Usage(_) => 2,
            CliError::NoDevice(_) => 3,
            CliError::Permission(_) => 4,
            CliError::Device(_) => 5,
        }
    }
}

impl From<session::Error> for CliError {
    fn from(e: session::Error) -> Self {
        match e {
            session::Error::Transport(t) => t.into(),
            other => CliError::Device(other.to_string()),
        }
    }
}

impl From<transport::Error> for CliError {
    fn from(e: transport::Error) -> Self {
        match e {
            transport::Error::Permission(_) => CliError::Permission(e.to_string()),
            transport::Error::NotFound => CliError::NoDevice(e.to_string()),
            other => CliError::Device(other.to_string()),
        }
    }
}

impl From<pdm::Error> for CliError {
    fn from(e: pdm::Error) -> Self {
        match e {
            pdm::Error::NotFound => CliError::NoDevice(e.to_string()),
            pdm::Error::Ambiguous { .. } | pdm::Error::HighVoltageNotConfirmed { .. } => {
                CliError::Usage(e.to_string())
            }
            other => CliError::Device(other.to_string()),
        }
    }
}

impl From<pdm::pd::Error> for CliError {
    fn from(e: pdm::pd::Error) -> Self {
        match e {
            pdm::pd::Error::Pdm(p) => p.into(),
            pdm::pd::Error::BadSettings { .. } => CliError::Device(e.to_string()),
            // Request validation: the user asked for something not offered.
            other => CliError::Usage(other.to_string()),
        }
    }
}

/// A failed write to an output sink (stdout, stderr or the CSV file).
impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Device(format!("write failed: {e}"))
    }
}
