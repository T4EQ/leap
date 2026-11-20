use std::{fmt::Display, ops::Deref};

use base64::prelude::*;
use itertools::Itertools;

const SHA256_BYTES: usize = 32;

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseSha256Error {
    #[error("Expected 64 hex characters, but found {0} characters")]
    UnexpectedCharacterCount(usize),
    #[error("Expected 32 decoded bytes, but found {0} bytes")]
    UnexpectedByteCount(usize),
    #[error("Expected only hex characters, but the string contains invalid character: {0}")]
    InvalidCharacter(char),
    #[error("Invalid base64 encoding: {0}")]
    Base64DecodingFailed(#[from] base64::DecodeError),
}

/// A SHA-256 hash represented as a hex string.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Sha256([u8; SHA256_BYTES]);

impl Display for Sha256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Sha256 {
    /// Returns the 32-byte representation of the SHA-256 hash.
    pub fn as_bytes(&self) -> &[u8; SHA256_BYTES] {
        &self.0
    }

    /// creates a Sha256 from a base64-encoded string containing the 32 bytes of the hash.
    pub fn try_from_base64(s: &str) -> Result<Self, ParseSha256Error> {
        let decoded = BASE64_STANDARD.decode(s)?;
        let len = decoded.len();
        let bytes = decoded
            .try_into()
            .map_err(|_| ParseSha256Error::UnexpectedByteCount(len))?;
        Ok(Self(bytes))
    }

    /// Returns the SHA-256 as a base64-encoded string.
    pub fn to_base64(&self) -> String {
        BASE64_STANDARD.encode(self.0)
    }

    /// Human-readable lowercase string of the SHA-256. Will contain exactly 64 characters.
    pub fn to_human_string(&self) -> String {
        self.0
            .iter()
            .flat_map(|byte| {
                let as_char =
                    |b| char::from_digit(b as u32, 16).expect("values are ranged from 0 to 16");
                let low_nibble = as_char(byte % 16);
                let high_nibble = as_char(byte / 16);
                [high_nibble, low_nibble].into_iter()
            })
            .collect()
    }

    /// Parses a SHA-256 from a 64-character human-readable hex string.
    pub fn try_from_human_string(s: &str) -> Result<Self, ParseSha256Error> {
        let to_digit = |c: char| {
            c.to_digit(16)
                .map(|e| e as u8)
                .ok_or(ParseSha256Error::InvalidCharacter(c))
        };

        let bytes: Vec<u8> = s
            .chars()
            .chunks(2)
            .into_iter()
            .map(|chunk| {
                let bytes: Vec<char> = chunk.collect();
                Ok(match bytes.deref() {
                    &[h, l] => (to_digit(h)? << 4) | to_digit(l)?,
                    _ => return Err(ParseSha256Error::UnexpectedCharacterCount(s.len())),
                })
            })
            .try_collect()?;

        Ok(Sha256(bytes.try_into().map_err(|_| {
            ParseSha256Error::UnexpectedCharacterCount(s.len())
        })?))
    }
}

impl TryFrom<&[u8]> for Sha256 {
    type Error = String;

    fn try_from(v: &[u8]) -> Result<Self, String> {
        let v: [u8; SHA256_BYTES] = v.try_into().map_err(|_| {
            format!(
                "Sha256 can only be constructed from a 32-byte slice. Got {} bytes",
                v.len()
            )
        })?;
        Ok(Sha256(v))
    }
}

impl<'de> serde::Deserialize<'de> for Sha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(DeserializeVisitor {})
    }
}

impl serde::Serialize for Sha256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let base64 = self.to_base64();
        serializer.serialize_str(&base64)
    }
}

pub struct DeserializeVisitor {}

impl<'de> serde::de::Visitor<'de> for DeserializeVisitor {
    type Value = Sha256;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a base64-encoded SHA-256 hash containing exactly 32 bytes")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Sha256::try_from_base64(v).map_err(|e| E::custom(e))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use googletest::prelude::*;
    use rstest::rstest;

    #[gtest]
    fn try_from_human_string() {
        expect_that!(
            Sha256::try_from_human_string(
                "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327"
            ),
            ok(property!(
                &Sha256.as_bytes(),
                eq(&[
                    0x0b, 0x88, 0xb2, 0xde, 0xc2, 0xbe, 0x5e, 0x2e, 0xf7, 0x40, 0x22, 0xef, 0x6a,
                    0x80, 0x23, 0x23, 0x2e, 0x28, 0x37, 0x4d, 0x67, 0xe9, 0x17, 0xb7, 0x6f, 0x9b,
                    0xb6, 0x07, 0xe6, 0x91, 0xf3, 0x27,
                ])
            ))
        );
    }

    #[gtest]
    #[rstest]
    #[case::empty("", ParseSha256Error::UnexpectedCharacterCount(0))]
    #[case::too_short(
        "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f32",
        ParseSha256Error::UnexpectedCharacterCount(63)
    )]
    #[case::too_long(
        "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f3233",
        ParseSha256Error::UnexpectedCharacterCount(65)
    )]
    #[case::invalid_char(
        "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f3=3",
        ParseSha256Error::InvalidCharacter('=')
    )]
    fn try_from_human_string_errors(#[case] input: &str, #[case] expected_error: ParseSha256Error) {
        expect_that!(
            Sha256::try_from_human_string(input),
            err(eq(&expected_error))
        );
    }

    #[gtest]
    fn try_from_base64() {
        expect_that!(
            Sha256::try_from_base64("C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8yc="),
            ok(property!(
                &Sha256.as_bytes(),
                eq(&[
                    0x0b, 0x88, 0xb2, 0xde, 0xc2, 0xbe, 0x5e, 0x2e, 0xf7, 0x40, 0x22, 0xef, 0x6a,
                    0x80, 0x23, 0x23, 0x2e, 0x28, 0x37, 0x4d, 0x67, 0xe9, 0x17, 0xb7, 0x6f, 0x9b,
                    0xb6, 0x07, 0xe6, 0x91, 0xf3, 0x27,
                ])
            ))
        );
    }

    #[gtest]
    #[rstest]
    #[case::invalid_char(
        "C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8yc&",
        ParseSha256Error::Base64DecodingFailed(base64::DecodeError::InvalidByte(43, b'&'))
    )]
    #[case::empty("", ParseSha256Error::UnexpectedByteCount(0))]
    #[case::too_short(
        "C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8w==",
        ParseSha256Error::UnexpectedByteCount(31)
    )]
    #[case::too_long(
        "C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8ycA",
        ParseSha256Error::UnexpectedByteCount(33)
    )]
    #[case::missing_padding(
        "C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8yc",
        ParseSha256Error::Base64DecodingFailed(base64::DecodeError::InvalidPadding)
    )]
    #[case::invalid_length(
        "A",
        ParseSha256Error::Base64DecodingFailed(base64::DecodeError::InvalidLength(1))
    )]
    #[case::nonzero_trailing_bits(
        "C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8yd=",
        ParseSha256Error::Base64DecodingFailed(base64::DecodeError::InvalidLastSymbol {
            offset: 42,
            symbol: b'd',
            symbol_value: 29,
        })
    )]
    fn try_from_base64_errors(#[case] input: &str, #[case] expected_error: ParseSha256Error) {
        expect_that!(Sha256::try_from_base64(input), err(eq(&expected_error)));
    }

    #[gtest]
    fn deserialize_sha256_error_messages() {
        let error =
            serde_json::from_str::<Sha256>(r#""C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8w==""#)
                .unwrap_err();
        expect_that!(
            error.to_string(),
            contains_substring("Expected 32 decoded bytes, but found 31 bytes")
        );

        let error = serde_json::from_str::<Sha256>("123").unwrap_err();
        expect_that!(
            error.to_string(),
            contains_substring("a base64-encoded SHA-256 hash containing exactly 32 bytes")
        );
    }

    #[gtest]
    fn to_base64() {
        expect_that!(
            Sha256::try_from_human_string(
                "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327"
            ),
            ok(property!(
                Sha256.to_base64(),
                eq(&"C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8yc=".to_string())
            ))
        );
    }

    #[gtest]
    fn to_human_string() {
        expect_that!(
            Sha256::try_from_base64("C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8yc="),
            ok(property!(
                Sha256.to_human_string(),
                eq(&"0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327".to_string())
            ))
        );
    }

    #[gtest]
    fn deserialize_sha256() -> googletest::Result<()> {
        let sha256 =
            serde_json::from_str::<Sha256>(r#""C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8yc=""#)
                .or_fail()?;
        expect_that!(
            sha256,
            eq(&Sha256::try_from_human_string(
                "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327"
            )
            .or_fail()?)
        );

        Ok(())
    }

    #[gtest]
    fn deserialize_sha256_incorrect_format() -> googletest::Result<()> {
        let testcases = [
            // Too short
            r#""C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8w==""#,
            // Too long
            r#""C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8ycA""#,
            // Empty
            r#""""#,
        ];

        for testcase in testcases {
            expect_that!(serde_json::from_str::<Sha256>(testcase), err(anything()));
        }

        Ok(())
    }

    #[gtest]
    fn serialize_sha256() -> googletest::Result<()> {
        let expected = r#""C4iy3sK+Xi73QCLvaoAjIy4oN01n6Re3b5u2B+aR8yc=""#;
        let sha256 = serde_json::to_string(
            &Sha256::try_from_human_string(
                "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327",
            )
            .or_fail()?,
        )
        .or_fail()?;
        expect_that!(sha256, eq(expected));
        Ok(())
    }
}
