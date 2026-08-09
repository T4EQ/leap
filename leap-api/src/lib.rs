//! The `leap-api` crate defines common data types shared by `leap-site` and `leap-server`.
//!
//! The create follows these conventions:
//! - Each API endpoint of the `leap-server` defines a full namespace path.
//! - For each endpoint namespace, `leap-api` defines a nested namespace
//!   with the API method of the endpoint.
//! - Inside the namespace for a given API endpoint, the following types are defined:
//!   - If the request method is `GET`, a `Query` type may be defined to indicate what query
//!     parameters can be sent to the server.
//!   - If the endpoint returns a JSON body, a `Response` type defines its contents.
//!   - Any additional types required to define either the query or the response.
//!
//! The supported endpoints are:
//!  - `POST` `api/manifest/fetch`. Triggers an immediate fetch of the manifest, causing the LEAP to
//!    update its cached content.
//!  - `GET` `api/manifest/latest`. Returns the latest manifest that is in use by the LEAP.
//!  - `GET` `api/content/meta`. Returns a list of the content metadata in the local server (LEAP).
//!  - `GET` `api/content/meta/{id}`. Returns the metadata of the requested id.
//!  - `GET` `api/content/{id}`. Obtains the requested content from the server. The path indicates
//!    the resource ID.

pub mod types;

pub mod api {
    pub mod version {
        pub mod get {
            pub use crate::types::BuildInfo;

            /// The response to the `GET` `api/version` request
            pub type Response = BuildInfo;
        }
    }

    pub mod content {
        pub mod meta {
            pub mod get {
                pub use crate::types::{
                    GroupedSection, LocalVideoMeta, ManifestMeta, Progress, VideoStatus,
                };

                /// The response to the `GET` `api/content/meta` request
                #[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq)]
                pub struct Response {
                    pub meta: Option<ManifestMeta>,
                }
            }

            pub mod id {
                pub mod get {
                    pub use crate::types::{LocalVideoMeta, Progress, VideoStatus};

                    /// The response to the `GET` `api/content/meta/{id}` request
                    #[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq)]
                    pub struct Response {
                        pub meta: Option<LocalVideoMeta>,
                    }
                }
            }
        }
    }
}

pub mod provision {
    pub mod network {
        pub mod post {
            pub use crate::types::{NetworkConfig, NetworkConfigResult};

            /// The request to the `POST` `provision/network` endpoint
            pub type Request = NetworkConfig;

            /// The response to the `POST` `provision/network` request
            pub type Response = NetworkConfigResult;
        }
    }

    pub mod storage {
        pub mod devices {
            pub mod get {
                pub use crate::types::BlockDevice;

                /// The request to the `GET` `provision/storage/devices` endpoint
                pub type Request = ();

                /// The response to the `GET` `provision/storage/devices` endpoint
                pub type Response = Vec<BlockDevice>;
            }
        }
    }

    pub mod config {
        pub mod post {
            pub use crate::types::{LeapConfig, LeapConfigResult};

            /// The request to the `POST` `provision/config` endpoint
            pub type Request = LeapConfig;

            /// The response to the `POST` `provision/config` request
            pub type Response = LeapConfigResult;
        }
    }

    pub mod status {
        pub mod get {
            pub use crate::types::ProvisionStatus;

            /// The request to the `GET` `provision/status` endpoint
            pub type Request = ();

            /// The response to the `GET` `provision/status` endpoint
            pub type Response = ProvisionStatus;
        }
    }
}
