//! Common data types used by the APIs
//!
use http::Uri;
use secrecy::{ExposeSecret, SecretString};
use std::{fmt::Display, net::Ipv4Addr};

mod parse_uri {
    //! Parses an [`http::Uri`] type using serde

    use http::Uri;

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<Uri, D::Error> {
        d.deserialize_str(Visitor {})
    }

    pub fn serialize<S>(data: &Uri, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = format!("{data}");
        serializer.serialize_str(&s)
    }

    struct Visitor {}

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Uri;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            writeln!(formatter, "A valid URI")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            v.try_into()
                .map_err(|e| E::custom(format!("{v} is an invalid URI: {e}")))
        }
    }
}

pub fn serialize_secret_str<S>(data: &SecretString, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(data.expose_secret())
}

/// Download progress. A number from 0 to 1, where 1 indicates completed and 0 not
/// started.
#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Clone)]
pub struct Progress(pub f64);

/// The status of the video download
#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Clone)]
pub enum VideoStatus {
    /// The video download has not started
    Pending,
    /// The video download is in progress
    Downloading(Progress),
    /// The video download is completed
    Downloaded,
    /// The video download failed
    Failed(String),
}

/// Metadata of a single video of the local server.
#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Clone)]
pub struct LocalVideoMeta {
    /// Unique identifier of the video
    pub id: String,
    /// Human-readable name of the video
    pub name: String,
    /// Size of the video in bytes
    pub size: usize,
    /// Download status
    pub status: VideoStatus,
    /// Total views of the video
    pub view_count: u64,
}

/// Grouped section of video content
#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Clone)]
pub struct GroupedSection {
    /// Name of the section
    pub name: String,

    /// Content within the section. Ordered as displayed
    pub content: Vec<LocalVideoMeta>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Clone)]
pub struct BuildInfo {
    pub name: String,
    pub version: String,
    pub git_hash: Option<String>,
    pub authors: Vec<String>,
    pub homepage: String,
    pub license: String,
    pub repository: String,
    pub profile: String,
    pub rustc_version: String,
    pub features: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct RetryParams {
    /// The initial backoff time after a download failure.
    #[serde(with = "humantime_serde")]
    pub initial_backoff: std::time::Duration,

    /// The adjustement factor for the backoff after a failure. Must be larger than 1 so that
    /// the backoff actually grows exponentially
    pub backoff_factor: f64,

    /// The maximum backoff time after a download failure.
    #[serde(with = "humantime_serde")]
    pub max_backoff: std::time::Duration,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct DownloaderConfig {
    /// Number of maximum concurrent downloads.
    pub concurrent_downloads: usize,

    /// The interval at which the remote is queried for new content.
    #[serde(with = "humantime_serde")]
    pub update_interval: std::time::Duration,

    /// Retry parameters when a download fails.
    pub retry_params: RetryParams,
}

/// Configuration to access the S3 server.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct S3Config {
    /// URI of the bucket
    #[serde(with = "parse_uri")]
    pub bucket: Uri,

    /// Access key ID.
    #[serde(serialize_with = "serialize_secret_str")]
    pub access_key_id: SecretString,

    /// Secret Access key.
    #[serde(serialize_with = "serialize_secret_str")]
    pub secret_access_key: SecretString,

    /// S3 Endpoint URL. Defaults to AWS if not given.
    pub endpoint_url: Option<String>,

    /// Uses path-style access to buckets instead of dns-based access. Use this if your endpoint is
    /// not AWS and you are unable to connect to your bucket (MinIO, Ceph, etc).
    pub force_path_style: Option<bool>,

    /// AWS region. Defaults to `us-east-1`.
    pub region: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct LeapConfig {
    /// Downloader service configuration.
    pub downloader_config: DownloaderConfig,

    /// S3 configuration.
    pub s3_config: S3Config,
}

pub type LeapConfigResult = std::result::Result<(), String>;

// At this time we only support Ipv4.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct StaticIpConfig {
    pub ip_address: Ipv4Addr,
    pub net_mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum IpConfig {
    Dhcp,
    Static(StaticIpConfig),
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct WirelessConfig {
    pub ssid: String,
    #[serde(serialize_with = "serialize_secret_str")]
    pub password: SecretString,
    pub ip_config: IpConfig,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct WiredConfig {
    pub ip_config: IpConfig,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum NetworkConfig {
    Wireless(WirelessConfig),
    Wired(WiredConfig),
}

impl NetworkConfig {
    pub fn is_wired(&self) -> bool {
        matches!(self, NetworkConfig::Wired(_))
    }

    pub fn is_wireless(&self) -> bool {
        matches!(self, NetworkConfig::Wireless(_))
    }
}

pub type NetworkConfigResult = std::result::Result<(), String>;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisionStatus {
    NetworkConfig,
    StorageConfig,
    LeapConfig,
    Completed,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
pub enum DeviceType {
    Disk,
    Partition,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
pub struct BlockDevice {
    pub name: String,
    pub size: u64,
    pub device_type: DeviceType,
}

impl Display for DeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceType::Partition => write!(f, "partition"),
            DeviceType::Disk => write!(f, "disk"),
        }
    }
}
