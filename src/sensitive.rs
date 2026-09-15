//! Redaction wrapper for secret configuration values.

use core::fmt;

/// Wraps a secret so `Debug` and `Display` always render `"[REDACTED]"`
/// while the value stays usable through [`Sensitive::expose`].
///
/// Deliberately attribute-free: no derive macros, so nothing can
/// accidentally drift into leaking. The redacting impls carry no `T:
/// Debug`/`T: Display` bound, which means a container's derived `Debug`
/// also renders `"[REDACTED]"` for the field — the value cannot escape
/// through any formatting path.
///
/// Deserialization is always supported (loading configuration is the
/// point). Serialization is opt-in behind the `serde` feature, because
/// serializing secrets invites accidental disclosure — enable it only when
/// your pipeline genuinely needs to write the value back out. Every
/// intentional read goes through [`expose`](Sensitive::expose), which makes
/// secret usage grep-friendly: `grep -rn '\.expose()'` audits every
/// touchpoint.
///
/// # Example
///
/// ```
/// use config_kit::Sensitive;
///
/// let token = Sensitive::new("hunter2".to_owned());
/// assert_eq!(format!("{token:?}"), "[REDACTED]");
/// assert_eq!(format!("{token}"), "[REDACTED]");
/// assert_eq!(token.expose(), "hunter2");
/// ```
pub struct Sensitive<T> {
    value: T,
}

impl<T> Sensitive<T> {
    /// Wraps `value` for redacted transport.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self { value }
    }

    /// Explicit, grep-friendly access to the wrapped secret.
    #[must_use]
    pub fn expose(&self) -> &T {
        &self.value
    }

    /// Consumes the wrapper, returning the secret. Like [`expose`](Self::expose),
    /// this is an explicit declassification point.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T> From<T> for Sensitive<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T> fmt::Debug for Sensitive<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl<T> fmt::Display for Sensitive<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl<'de, T> serde::Deserialize<'de> for Sensitive<T>
where
    T: serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        T::deserialize(deserializer).map(Sensitive::new)
    }
}

/// Serialization is opt-in: see the type-level documentation for why.
#[cfg(feature = "serde")]
impl<T> serde::Serialize for Sensitive<T>
where
    T: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.value.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn debug_and_display_redact() {
        let secret = Sensitive::new("hunter2".to_owned());
        assert_eq!(format!("{secret:?}"), "[REDACTED]");
        assert_eq!(format!("{secret}"), "[REDACTED]");
        assert!(!format!("{secret:?}").contains("hunter2"));
    }

    #[test]
    fn access_is_explicit() {
        let secret = Sensitive::from(42_u32);
        assert_eq!(*secret.expose(), 42);
        assert_eq!(secret.into_inner(), 42);
    }

    #[test]
    fn deserializes_and_redacts_through_containers() {
        #[derive(serde::Deserialize, Debug)]
        struct Container {
            token: Sensitive<String>,
        }

        let container: Container =
            toml::from_str("token = \"top-secret\"").expect("deserialize container");
        assert_eq!(container.token.expose(), "top-secret");
        let rendered = format!("{container:?}");
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
        assert!(!rendered.contains("top-secret"), "{rendered}");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip_preserves_value() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrapper {
            token: Sensitive<String>,
        }

        let original = Wrapper {
            token: Sensitive::new("rotate-me".to_owned()),
        };
        let encoded = toml::to_string(&original).expect("serialize");
        let decoded: Wrapper = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded.token.expose(), "rotate-me");
        assert_eq!(format!("{:?}", decoded.token), "[REDACTED]");
    }
}
