use crate::bitstream::BitError;

/// Errors produced by the decoder.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The bitstream ended early or contained an invalid Exp-Golomb code.
    #[error("bitstream error: {0}")]
    Bitstream(#[from] BitError),
    /// A syntax element had a value outside its legal range.
    #[error("invalid bitstream: {0}")]
    Invalid(String),
    /// A valid feature this decoder does not implement (yet).
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// A slice referenced a parameter set that has not been received.
    #[error("missing parameter set: {0}")]
    MissingParameterSet(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(msg.into()))
}

pub(crate) fn unsupported<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Unsupported(msg.into()))
}

/// `ensure!(cond, "message")` returns `Error::Invalid` when `cond` is false.
macro_rules! ensure {
    ($cond:expr, $($arg:tt)+) => {
        if !$cond {
            return Err($crate::h264::error::Error::Invalid(format!($($arg)+)));
        }
    };
}
pub(crate) use ensure;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_error_variants_have_stable_classification_and_messages() {
        let cases = [
            (Error::Invalid("bad slice".into()), "invalid bitstream: bad slice"),
            (Error::Unsupported("profile".into()), "unsupported: profile"),
            (Error::MissingParameterSet("sps 7".into()), "missing parameter set: sps 7"),
            (Error::Bitstream(BitError::Eof), "bitstream error: unexpected end of bitstream"),
            (Error::Bitstream(BitError::InvalidExpGolomb), "bitstream error: invalid Exp-Golomb code"),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
            assert_eq!(error.clone(), error);
        }
    }

    #[test]
    fn error_helpers_and_ensure_macro_preserve_invalid_vs_unsupported() {
        assert_eq!(invalid::<()>("out of range"), Err(Error::Invalid("out of range".into())));
        assert_eq!(unsupported::<()>("feature"), Err(Error::Unsupported("feature".into())));
        fn validate(value: i32) -> Result<i32> {
            ensure!(value >= 0, "negative value: {value}");
            Ok(value)
        }
        assert_eq!(validate(0), Ok(0));
        assert_eq!(validate(-3), Err(Error::Invalid("negative value: -3".into())));
    }

    #[test]
    fn bitstream_error_conversion_is_lossless() {
        for source in [BitError::Eof, BitError::InvalidExpGolomb] {
            let decoded: Error = source.into();
            assert_eq!(decoded, Error::Bitstream(source));
        }
    }
}
