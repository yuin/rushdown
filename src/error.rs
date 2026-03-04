//! Custom error types for the rushdown library.

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use core::fmt::{self, Debug, Formatter};
use core::result::Result as CoreResult;
use core::{error::Error as CoreError, fmt::Display};

use crate::ast::NodeRef;

#[cfg(feature = "std")]
use std::backtrace::{Backtrace, BacktraceStatus};

/// Alias for a Result type that uses [`Error`].
pub type Result<T> = CoreResult<T, Error>;

/// Error type that can represent either an internal [`Error`] or a user-defined callback
/// error.
#[derive(Debug)]
#[non_exhaustive]
pub enum CallbackError<E: CoreError + 'static> {
    /// Internal error from the rushdown library.
    Internal(Error),

    /// User-defined callback error.
    Callback(E),
}

impl<E: CoreError + 'static> Display for CallbackError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            CallbackError::Internal(err) => write!(f, "{}", err),
            CallbackError::Callback(err) => {
                write!(f, "{}", err)
            }
        }
    }
}

impl<E: CoreError + 'static> CoreError for CallbackError<E> {
    fn source(&self) -> Option<&(dyn CoreError + 'static)> {
        match self {
            CallbackError::Internal(err) => Some(err),
            CallbackError::Callback(err) => Some(err),
        }
    }
}

/// Custom error type for the rushdown library.
#[non_exhaustive]
pub enum Error {
    /// Invalid node reference error.
    InvalidNodeRef {
        noderef: NodeRef,
        description: String,
        #[cfg(feature = "std")]
        backtrace: Backtrace,
    },
    /// Invalid node operation error.
    InvalidNodeOperation {
        message: String,
        description: String,
        #[cfg(feature = "std")]
        backtrace: Backtrace,
    },
    /// I/O error.
    Io {
        message: String,
        description: String,
        source: Option<Box<dyn CoreError + 'static>>,
        #[cfg(feature = "std")]
        backtrace: Backtrace,
    },
}

impl Error {
    /// Creates a new invalid node reference error.
    pub fn invalid_node_ref(node_ref: NodeRef) -> Self {
        Error::InvalidNodeRef {
            noderef: node_ref,
            description: format!("invalid node reference: {}", node_ref),
            #[cfg(feature = "std")]
            backtrace: Backtrace::capture(),
        }
    }

    /// Creates a new invalid operation error with a message.
    pub fn invalid_node_operation(message: String) -> Self {
        Error::InvalidNodeOperation {
            message: message.clone(),
            description: format!("invalid operation: {}", message),
            #[cfg(feature = "std")]
            backtrace: Backtrace::capture(),
        }
    }

    /// Creates a new I/O error with an optional source error.
    pub fn io<S>(m: S, source: Option<Box<dyn CoreError + 'static>>) -> Self
    where
        S: Into<String>,
    {
        let message = m.into();
        Error::Io {
            message: message.clone(),
            description: format!("io error: {}", message),
            source,
            #[cfg(feature = "std")]
            backtrace: Backtrace::capture(),
        }
    }

    /// Returns the backtrace associated with the error, if available.
    /// This is only available when the `std` feature is enabled.
    #[cfg(feature = "std")]
    pub fn backtrace(&self) -> Option<&Backtrace> {
        match self {
            Error::InvalidNodeRef { backtrace, .. } => Some(backtrace),
            Error::InvalidNodeOperation { backtrace, .. } => Some(backtrace),
            Error::Io { backtrace, .. } => Some(backtrace),
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidNodeRef { description, .. } => write!(f, "{}", description),
            Error::InvalidNodeOperation { description, .. } => write!(f, "{}", description),
            Error::Io { description, .. } => write!(f, "{}", description),
        }
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidNodeRef {
                description,
                #[cfg(feature = "std")]
                backtrace,
                ..
            } => {
                write!(f, "{}", description)?;
                #[cfg(feature = "std")]
                {
                    write!(f, "{}", format_backtrace(backtrace))?;
                }
            }
            Error::InvalidNodeOperation {
                description,
                #[cfg(feature = "std")]
                backtrace,
                ..
            } => {
                write!(f, "{}", description)?;
                #[cfg(feature = "std")]
                {
                    write!(f, "{}", format_backtrace(backtrace))?;
                }
            }
            Error::Io {
                description,
                #[cfg(feature = "std")]
                backtrace,
                ..
            } => {
                write!(f, "{}", description)?;
                #[cfg(feature = "std")]
                {
                    write!(f, "{}", format_backtrace(backtrace))?;
                }
            }
        }
        if let Some(source) = self.source() {
            writeln!(f, "Caused by: {:?}", source)?;
        }
        Ok(())
    }
}

impl CoreError for Error {
    fn source(&self) -> Option<&(dyn CoreError + 'static)> {
        {
            match self {
                Error::Io { source, .. } => source.as_deref(),
                _ => None,
            }
        }
    }
}

#[cfg(feature = "std")]
fn format_backtrace(backtrace: &Backtrace) -> String {
    match backtrace.status() {
        BacktraceStatus::Captured => format!("\nstack backtrace:\n{}", backtrace),
        _ => String::new(),
    }
}
