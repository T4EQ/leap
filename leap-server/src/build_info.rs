//! Build information for the LEAP server.
//!
//! This module provides access to metadata compiled into the binary at build time,
//! such as the version, git hash, and build profile.
//!
//! The information is typically generated via a `build.rs` script and included
//! using `std::include!`.

/// Information about the current build of the LEAP server.
#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Clone)]
pub struct BuildInfo {
    /// The name of the project.
    pub name: &'static str,
    /// The version of the project.
    pub version: &'static str,
    /// The git commit hash of the build, if available.
    /// If the repository was dirty, it will be suffixed with `-dirty`.
    pub git_hash: Option<String>,
    /// A list of authors of the project.
    pub authors: Vec<String>,
    /// The homepage URL of the project.
    pub homepage: &'static str,
    /// The license of the project.
    pub license: &'static str,
    /// The repository URL of the project.
    pub repository: &'static str,
    /// The build profile used (e.g., "debug", "release").
    pub profile: &'static str,
    /// The version of the Rust compiler used for the build.
    pub rustc_version: &'static str,
    /// A string representation of the enabled Cargo features.
    pub features: &'static str,
}

std::include!(std::concat!(std::env!("OUT_DIR"), "/built.rs"));

#[cfg(feature = "git2")]
fn git_hash() -> Option<String> {
    GIT_COMMIT_HASH.map(|git_hash| {
        let dirty = if GIT_DIRTY.is_some_and(|v| v) {
            "-dirty"
        } else {
            ""
        };
        format!("{git_hash}{dirty}")
    })
}

#[cfg(not(feature = "git2"))]
fn git_hash() -> Option<String> {
    std::option_env!("LEAP_SERVER_NIX_GIT_REVISION").map(|git_hash| git_hash.to_string())
}

/// Retrieves the build information for the current binary.
///
/// This function returns a [`BuildInfo`] struct containing the metadata
/// compiled into the binary.
pub fn get() -> BuildInfo {
    let authors = PKG_AUTHORS
        .split(':')
        .map(|author| author.trim().to_string())
        .collect();
    BuildInfo {
        name: PKG_NAME,
        version: PKG_VERSION,
        git_hash: git_hash(),
        authors,
        homepage: PKG_HOMEPAGE,
        license: PKG_LICENSE,
        repository: PKG_REPOSITORY,
        profile: PROFILE,
        rustc_version: RUSTC_VERSION,
        features: FEATURES_STR,
    }
}
