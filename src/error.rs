use std::fmt;

/// Opaque error from [`crate::PowerWatch::start`].
#[derive(Debug)]
pub struct Error {
    message: &'static str,
}

impl Error {
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) const fn unsupported() -> Self {
        Self {
            message: "powerwatch is not supported on this platform",
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) const fn platform(message: &'static str) -> Self {
        Self { message }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}

impl std::error::Error for Error {}
