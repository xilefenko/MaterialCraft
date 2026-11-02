//! `format_version`: `major.minor`. A reader rejects a newer major version and reads any minor
//! version of its own major (newer minors only add fields, which are kept on re-save).

use super::FormatError;
use std::cmp::Ordering;
use std::fmt;

/// A parsed `format_version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    /// The version this code writes ([`super::FORMAT_VERSION`]).
    pub const CURRENT: Version = Version { major: 0, minor: 18 };

    /// Parses `major.minor` (decimal, no sign, no leading zeros except `0` itself).
    pub fn parse(text: &str) -> Option<Version> {
        let (major, minor) = text.split_once('.')?;
        Some(Version { major: number(major)?, minor: number(minor)? })
    }

    /// Checks that a file of version `text` can be read by this code.
    pub fn check_readable(text: &str) -> Result<Version, FormatError> {
        let v = Version::parse(text).ok_or_else(|| FormatError::manifest("format_version", format!("{text:?} is not major.minor")))?;
        match v.major.cmp(&Version::CURRENT.major) {
            Ordering::Greater => Err(FormatError::NewerVersion { found: text.to_string(), supported: format!("{}.x", Version::CURRENT.major) }),
            Ordering::Less => Err(FormatError::Unsupported(format!("format version {text} is older than this program reads"))),
            Ordering::Equal => Ok(v),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

fn number(s: &str) -> Option<u32> {
    let ok = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    ok.then(|| s.parse().ok()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_check() {
        assert_eq!(Version::CURRENT.to_string(), super::super::FORMAT_VERSION);
        assert_eq!(Version::parse("0.1"), Some(Version { major: 0, minor: 1 }));
        for bad in ["", "1", "1.", ".1", "01.0", "1.0.0", "+1.0", "a.b", "1.-1"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert!(Version::check_readable("0.1").is_ok());
        assert!(Version::check_readable("0.19").is_ok(), "newer minor is read");
        assert!(matches!(Version::check_readable("1.0"), Err(FormatError::NewerVersion { .. })));
        assert!(matches!(Version::check_readable("x"), Err(FormatError::Manifest { .. })));
    }
}
