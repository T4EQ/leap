//! This module defines the manifest file structure used to describe sets of videos
//! and sections for the LEAP system.
//!
//! The manifest includes versioning, metadata for each video, and groups videos into sections.
//! It provides serialization and deserialization support for JSON.

use std::{fmt::Display, ops::Deref};

/// Version data type made of major, minor and revision numbers.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Version {
    /// Major version number.
    pub major: u32,
    /// Minor version number.
    pub minor: u32,
    /// Revision number.
    pub revision: u32,
}

/// A SHA-256 hash represented as a hex string.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Sha256(String);

impl Display for Sha256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Sha256 {
    /// Returns the 32-byte representation of the SHA-256 hash.
    pub fn as_bytes(&self) -> [u8; 32] {
        (0..32)
            .map(|byte_idx| {
                u8::from_str_radix(&self.0[2 * byte_idx..2 * byte_idx + 2], 16)
                    .expect("Sha256 should be a valid hex string of 64 chars")
            })
            .collect::<Vec<u8>>()
            .try_into()
            .expect("Sha256 can only be constructed with 64 characters")
    }
}

impl TryFrom<&[u8]> for Sha256 {
    type Error = String;

    fn try_from(v: &[u8]) -> Result<Self, String> {
        if v.len() != 32 {
            return Err(format!(
                "Sha256 can only be constructed from a 32-byte slice. Got {} bytes",
                v.len()
            ));
        }

        Ok(Sha256(
            v.iter()
                .flat_map(|byte| {
                    let msb = char::from_digit((byte >> 4) as u32, 16).unwrap();
                    let lsb = char::from_digit((byte & 0x0f) as u32, 16).unwrap();
                    std::iter::once(msb).chain(std::iter::once(lsb))
                })
                .collect(),
        ))
    }
}

impl TryFrom<&str> for Sha256 {
    type Error = String;

    fn try_from(v: &str) -> Result<Self, String> {
        use regex::Regex;
        use std::sync::LazyLock;
        static SHA_REGEX: LazyLock<Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new("^[0-9a-f]{64}$").expect("Invalid sha256 regex")
        });

        if !SHA_REGEX.is_match(v) {
            return Err(format!("\"{v}\" is not a valid SHA-256"));
        };

        Ok(Self(v.to_string()))
    }
}

impl Deref for Sha256 {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Metadata for a single video entry
#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq, Clone)]
pub struct Video {
    /// Human-readable name of the video
    pub name: String,

    /// Unique identifier of the video
    pub id: uuid::Uuid,

    /// Unique resource identifier from which the video can be downloaded
    #[serde(deserialize_with = "deserialize_uri")]
    #[serde(serialize_with = "serialize_uri")]
    pub uri: http::Uri,

    /// SHA-256 of the video file
    pub sha256: Sha256,

    /// File size in bytes
    pub file_size: u64,
}

/// A section of content that groups together a number of videos
#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq, Clone)]
pub struct Section {
    /// Name of the section
    pub name: String,

    /// Content within the section. Ordered as displayed
    pub content: Vec<Video>,
}

/// Describes the set of videos and sections to be shown in the LEAP.
#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq, Clone)]
pub struct ManifestFile {
    /// Name of the distribution list
    pub name: String,

    /// Date in which this manifest was released
    pub date: chrono::NaiveDate,

    /// Version of the manifest. At the moment only version 1.0.0 is supported
    pub version: Version,

    /// Sections in the manifest. Ordered as displayed
    pub sections: Vec<Section>,
}

fn serialize_uri<S>(uri: &http::Uri, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let s = uri.to_string();
    serializer.serialize_str(&s)
}

fn deserialize_uri<'de, D>(deserializer: D) -> Result<http::Uri, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_str(uri::Visitor {})
}

impl<'de> serde::Deserialize<'de> for Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(version::Visitor {})
    }
}

impl serde::Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("v{}.{}.{}", self.major, self.minor, self.revision))
    }
}

impl<'de> serde::Deserialize<'de> for Sha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(sha256::Visitor {})
    }
}

impl serde::Serialize for Sha256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self)
    }
}

mod uri {
    pub struct Visitor {}

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = http::Uri;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("A URI")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            v.parse().map_err(E::custom)
        }
    }
}

mod version {
    pub struct Visitor {}

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = super::Version;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("A version number of the form vX.Y.Z")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            use regex::Regex;
            use std::sync::LazyLock;
            static VERSION_REGEX: LazyLock<Regex> = std::sync::LazyLock::new(|| {
                regex::Regex::new("^v(\\d+)\\.(\\d+)\\.(\\d+)$").expect("Invalid version regex")
            });

            let Some(captures) = VERSION_REGEX.captures(v) else {
                return Err(E::custom("Invalid version string."));
            };

            let components: Result<Vec<u32>, _> = captures
                .iter()
                .skip(1) // This is the whole version match, we are not interested.
                .map(|c| {
                    c.expect("This capture must be present, it is not optional")
                        .as_str()
                        .parse()
                })
                .collect();

            let components = components.map_err(|_| E::custom("Invalid version string."))?;
            if components.len() != 3 {
                return Err(E::custom("Invalid version string."));
            }

            Ok(super::Version {
                major: components[0],
                minor: components[1],
                revision: components[2],
            })
        }
    }
}

mod sha256 {
    pub struct Visitor {}

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = super::Sha256;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("A Sha256 hash with 64 alfanumeric characters.")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            v.try_into().map_err(|e| E::custom(e))
        }
    }
}

#[cfg(test)]
pub mod test {
    use std::str::FromStr;

    use super::*;
    use googletest::prelude::*;

    pub fn new_version(major: u32, minor: u32, revision: u32) -> Version {
        Version {
            major,
            minor,
            revision,
        }
    }

    #[googletest::gtest]
    fn deserialize_version() -> googletest::Result<()> {
        let version = serde_json::from_str::<Version>(r#""v1.2.3""#).or_fail()?;
        expect_that!(version, eq(&new_version(1, 2, 3)));
        let version = serde_json::from_str::<Version>(r#""v432.224.8234""#).or_fail()?;
        expect_that!(version, eq(&new_version(432, 224, 8234)));

        Ok(())
    }

    #[googletest::gtest]
    fn deserialize_version_incorrect_format() -> googletest::Result<()> {
        let testcases = [
            r#""1.2.3""#,
            r#""v.2.3""#,
            r#""va1.2.3""#,
            r#""v1..3""#,
            r#""v1.3.3.4""#,
            r#""v1.3.""#,
            r#""v1.3""#,
            r#""a1.3a""#,
            r#""v1.3.3.a""#,
            r#""v1.3.3a""#,
        ];

        for testcase in testcases {
            expect_that!(serde_json::from_str::<Version>(testcase), err(anything()));
        }

        Ok(())
    }

    #[googletest::gtest]
    fn serialize_version() -> googletest::Result<()> {
        let expected = r#""v1.2.3""#;
        let version = serde_json::to_string(&new_version(1, 2, 3)).or_fail()?;
        expect_that!(version, eq(expected));
        Ok(())
    }

    #[googletest::gtest]
    fn deserialize_sha256() -> googletest::Result<()> {
        let sha256 = serde_json::from_str::<Sha256>(
            r#""0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327""#,
        )
        .or_fail()?;
        expect_that!(
            sha256,
            eq(&Sha256(
                "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327".to_string()
            ))
        );

        Ok(())
    }

    #[googletest::gtest]
    fn deserialize_sha256_incorrect_format() -> googletest::Result<()> {
        let testcases = [
            // Too short
            r#""b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327""#,
            // Too long
            r#""0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f3274""#,
            // Empty
            r#""""#,
            // Invalid characters
            r#""0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917g76f9bb607e691f327""#,
        ];

        for testcase in testcases {
            expect_that!(serde_json::from_str::<Version>(testcase), err(anything()));
        }

        Ok(())
    }

    #[googletest::gtest]
    fn serialize_sha256() -> googletest::Result<()> {
        let expected = r#""0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327""#;
        let sha256 = serde_json::to_string(&Sha256(
            "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327".to_string(),
        ))
        .or_fail()?;
        expect_that!(sha256, eq(expected));
        Ok(())
    }

    #[googletest::gtest]
    fn deserialize_video() -> googletest::Result<()> {
        let serialized = r#"{
            "name": "Linear equations",
            "id": "bf978778-1c5d-44b3-b2c1-1cc253563799",
            "uri": "s3://bucket/linear-equations.mp4",
            "sha256": "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327",
            "file_size": 123456
        }"#;

        let video: Video = serde_json::from_str(serialized).unwrap();
        expect_that!(
            video,
            eq(&Video {
                name: "Linear equations".to_string(),
                id: uuid::Uuid::from_str("bf978778-1c5d-44b3-b2c1-1cc253563799").or_fail()?,
                uri: "s3://bucket/linear-equations.mp4".parse().or_fail()?,
                sha256: Sha256(
                    "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327".to_string()
                ),
                file_size: 123456,
            })
        );
        Ok(())
    }

    #[googletest::gtest]
    fn deserialize_section() -> googletest::Result<()> {
        let serialized = r#"{
            "name": "Equations",
            "content": [
                {
                    "name": "Linear equations",
                    "id": "bf978778-1c5d-44b3-b2c1-1cc253563799",
                    "uri": "s3://bucket/linear-equations.mp4",
                    "sha256": "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327",
                    "file_size": 123456
                },
                {
                    "name": "Quadratic equations",
                    "id": "5eb9e089-79cf-478d-9121-9ca3e7bb1d4a",
                    "uri": "s3://bucket/quadratic-equations.mp4",
                    "sha256": "8f9e3a4ae7d86c4abdf731a947fc90b607b82a0362da0b312e3b644defedb81f",
                    "file_size": 123457
                },
                {
                    "name": "Cubic equations",
                    "id": "9e0f44b6-3dc6-4f56-8c9f-7e28feac1d03",
                    "uri": "s3://bucket/cubic-equations.mp4",
                    "sha256": "8b9522ce42fb02dd100b575714d935a4502872afccee80f7a65d466389a5bef8",
                    "file_size": 123458
                }
            ]
        }"#;

        let section: Section = serde_json::from_str(serialized).unwrap();
        expect_that!(
            section,
            eq(&Section {
                name: "Equations".to_string(),
                content: vec![
                    Video {
                        name: "Linear equations".to_string(),
                        id: uuid::Uuid::from_str("bf978778-1c5d-44b3-b2c1-1cc253563799")
                            .or_fail()?,
                        uri: "s3://bucket/linear-equations.mp4".parse().or_fail()?,
                        sha256: Sha256(
                            "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327"
                                .to_string()
                        ),
                        file_size: 123456,
                    },
                    Video {
                        name: "Quadratic equations".to_string(),
                        id: uuid::Uuid::from_str("5eb9e089-79cf-478d-9121-9ca3e7bb1d4a")
                            .or_fail()?,
                        uri: "s3://bucket/quadratic-equations.mp4".parse().or_fail()?,
                        sha256: Sha256(
                            "8f9e3a4ae7d86c4abdf731a947fc90b607b82a0362da0b312e3b644defedb81f"
                                .to_string()
                        ),
                        file_size: 123457,
                    },
                    Video {
                        name: "Cubic equations".to_string(),
                        id: uuid::Uuid::from_str("9e0f44b6-3dc6-4f56-8c9f-7e28feac1d03")
                            .or_fail()?,
                        uri: "s3://bucket/cubic-equations.mp4".parse().or_fail()?,
                        sha256: Sha256(
                            "8b9522ce42fb02dd100b575714d935a4502872afccee80f7a65d466389a5bef8"
                                .to_string()
                        ),
                        file_size: 123458,
                    },
                ]
            })
        );
        Ok(())
    }

    #[googletest::gtest]
    fn deserialize_manifest() -> googletest::Result<()> {
        let serialized = r#"{
    "name": "High school video distribution list",
    "date": "2025-10-10",
    "version": "v1.0.0",
    "sections": [
        {
            "name": "Equations",
            "content": [
                {
                    "name": "Linear equations",
                    "id": "bf978778-1c5d-44b3-b2c1-1cc253563799",
                    "uri": "s3://bucket/linear-equations.mp4",
                    "sha256": "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327",
                    "file_size": 123456
                },
                {
                    "name": "Quadratic equations",
                    "id": "5eb9e089-79cf-478d-9121-9ca3e7bb1d4a",
                    "uri": "s3://bucket/quadratic-equations.mp4",
                    "sha256": "8f9e3a4ae7d86c4abdf731a947fc90b607b82a0362da0b312e3b644defedb81f",
                    "file_size": 123457
                },
                {
                    "name": "Cubic equations",
                    "id": "9e0f44b6-3dc6-4f56-8c9f-7e28feac1d03",
                    "uri": "s3://bucket/cubic-equations.mp4",
                    "sha256": "8b9522ce42fb02dd100b575714d935a4502872afccee80f7a65d466389a5bef8",
                    "file_size": 123458
                }
            ]
        },
        {
            "name": "Integration",
            "content": [
                {
                    "name": "Riemann sum",
                    "id": "eddb4450-a9ff-4a4b-ad81-2a8b78998405",
                    "uri": "s3://bucket/riemann-sum.mp4",
                    "sha256": "a6d3b80cd14f78b21ffbf5995bbda38ad8834459557782d245ed720134d36fc4",
                    "file_size": 123459
                },
                {
                    "name": "List of integrals",
                    "id": "f47e6cdc-1bcf-439a-9ea4-038dc7153648",
                    "uri": "s3://bucket/list-of-integrals.mp4",
                    "sha256": "98780990e94fb55d0b88ebcd78fe82f069eac547731a4b0822332d826c970aec",
                    "file_size": 123460
                }
            ]
        }
    ]
}"#;

        let manifest: ManifestFile = serde_json::from_str(serialized).or_fail()?;
        expect_that!(
            manifest,
            eq(&ManifestFile {
                name: "High school video distribution list".to_string(),
                date: chrono::NaiveDate::from_str("2025-10-10").or_fail()?,
                version: new_version(1, 0, 0),
                sections: vec![
                    Section {
                        name: "Equations".to_string(),
                        content: vec![
                        Video {
                            name: "Linear equations".to_string(),
                            id: uuid::Uuid::from_str("bf978778-1c5d-44b3-b2c1-1cc253563799")
                                .or_fail()?,
                            uri: "s3://bucket/linear-equations.mp4".parse().or_fail()?,
                            sha256:
                                Sha256("0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327"
                                    .to_string()),
                            file_size: 123456,
                        },
                        Video {
                            name: "Quadratic equations".to_string(),
                            id: uuid::Uuid::from_str("5eb9e089-79cf-478d-9121-9ca3e7bb1d4a")
                                .or_fail()?,
                            uri: "s3://bucket/quadratic-equations.mp4".parse().or_fail()?,
                            sha256:
                                Sha256("8f9e3a4ae7d86c4abdf731a947fc90b607b82a0362da0b312e3b644defedb81f"
                                    .to_string()),
                            file_size: 123457,
                        },
                        Video {
                            name: "Cubic equations".to_string(),
                            id: uuid::Uuid::from_str("9e0f44b6-3dc6-4f56-8c9f-7e28feac1d03")
                                .or_fail()?,
                            uri: "s3://bucket/cubic-equations.mp4".parse().or_fail()?,
                            sha256:
                                Sha256("8b9522ce42fb02dd100b575714d935a4502872afccee80f7a65d466389a5bef8"
                                    .to_string()),
                            file_size: 123458,
                        },
                    ]
                    },
                    Section {
                        name: "Integration".to_string(),
                        content: vec![
                        Video {
                            name: "Riemann sum".to_string(),
                            id: uuid::Uuid::from_str("eddb4450-a9ff-4a4b-ad81-2a8b78998405")
                                .or_fail()?,
                            uri: "s3://bucket/riemann-sum.mp4".parse().or_fail()?,
                            sha256:
                                Sha256("a6d3b80cd14f78b21ffbf5995bbda38ad8834459557782d245ed720134d36fc4"
                                    .to_string()),
                            file_size: 123459,
                        },
                        Video {
                            name: "List of integrals".to_string(),
                            id: uuid::Uuid::from_str("f47e6cdc-1bcf-439a-9ea4-038dc7153648")
                                .or_fail()?,
                            uri: "s3://bucket/list-of-integrals.mp4".parse().or_fail()?,
                            sha256:
                                Sha256("98780990e94fb55d0b88ebcd78fe82f069eac547731a4b0822332d826c970aec"
                                    .to_string()),
                            file_size: 123460,
                        },
                    ]
                    }
                ],
            })
        );
        Ok(())
    }
}
