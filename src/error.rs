//! Errors.

use std::ffi::CString;

use thiserror::Error;

/// Errors from the hysteresis filter.
#[derive(Debug, Error)]
pub(crate) enum HysteresisError {
  /// A clip argument could not be read.
  #[error("failed to get {0}")]
  MissingClip(&'static str),

  /// The input format is not supported.
  #[error("only constant format 8-16 bits integer and 32 bits float input supported")]
  UnsupportedFormat,

  /// The clips differ in dimensions or format.
  #[error("both clips must have the same dimensions and format")]
  MismatchedClips,

  /// The `planes` argument could not be read.
  #[error("failed to read 'planes' parameter")]
  InvalidPlanes,

  /// A plane index is outside the valid range.
  #[error("plane index {0} is out of range [0, 3)")]
  PlaneOutOfRange(i32),

  /// A plane index was given more than once.
  #[error("plane {0} is specified more than once")]
  DuplicatePlane(i32),

  /// The filter dependencies could not be constructed.
  #[error("too many dependencies")]
  TooManyDependencies,

  /// The peak value cannot be represented in the sample type.
  #[error("peak value does not fit the sample type")]
  PeakOutOfRange,
}

impl From<HysteresisError> for CString {
  fn from(e: HysteresisError) -> Self {
    Self::new(format!("hysteresis: {e}"))
      .unwrap_or_else(|_| c"hysteresis: invalid error message".into())
  }
}
