//! This module contains the status page component.
//!
//! The status page provides a comprehensive overview of the system's current state, including:
//! - Build information (version, authors, etc.)
//! - Current manifest information
//! - Pending downloads
//! - System logs

use crate::context::ContentContextHandle;

use gloo_net::http::Request;
use leap_api::api::content::meta::get::VideoStatus;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use leap_api::api::version::get::BuildInfo;

#[derive(PartialEq, Clone)]
pub struct DownloadItem {
    pub id: String,
    pub name: String,
    pub status: VideoStatus,
}

#[derive(Debug, serde::Deserialize, PartialEq, Eq, Clone)]
pub struct ManifestInfo {
    pub name: String,
    pub date: String,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub message: String,
    pub kv_pairs: Vec<(String, String)>,
}

impl<'de> serde::Deserialize<'de> for LogEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
        D::Error: serde::de::Error,
    {
        let v = serde_json::Value::deserialize(deserializer)?;
        let timestamp = v
            .get("time")
            .ok_or(serde::de::Error::custom("Log does not have a timestamp"))?
            .as_str()
            .ok_or(serde::de::Error::custom("Log timestamp is not a string"))?
            .to_string();

        let level = match v
            .get("level")
            .ok_or(serde::de::Error::custom("Log does not have a log level"))?
            .as_u64()
            .ok_or(serde::de::Error::custom("Log level is not an int"))?
        {
            10 => LogLevel::Trace,
            20 => LogLevel::Debug,
            30 => LogLevel::Info,
            40 => LogLevel::Warn,
            50 => LogLevel::Error,
            60 => LogLevel::Fatal,
            l => {
                return Err(serde::de::Error::custom(format!("Invalid log level: {l}")));
            }
        };

        let message = v
            .get("msg")
            .ok_or(serde::de::Error::custom("Log does not have a message"))?
            .as_str()
            .ok_or(serde::de::Error::custom("Log message is not a string"))?
            .to_string();

        let kv_pairs = v
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| *k != "time" && *k != "level" && *k != "msg")
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();

        Ok(Self {
            timestamp,
            level,
            message,
            kv_pairs,
        })
    }
}

struct Status {
    version: BuildInfo,
    logs: Vec<LogEntry>,
    manifest: Option<(String, ManifestInfo)>,
    pending_downloads: Vec<DownloadItem>,
}

#[derive(Properties, PartialEq)]
pub struct ManifestStatusProps {
    pub manifest: Option<(String, ManifestInfo)>,
    pub on_fetch: Callback<MouseEvent>,
}

#[function_component(ManifestStatus)]
pub fn manifest_status(ManifestStatusProps { manifest, on_fetch }: &ManifestStatusProps) -> Html {
    let has_manifest = manifest.is_some();
    html! {
        <div class="status-section">
            <h2>{ "Current Manifest" }</h2>
            <div class="card details-card">
                <div class="details">
                {
                    if let Some((_, manifest_info)) = manifest {
                        html! {
                            <>
                            <div class="row">
                                <span class="label">{ "Name: " }</span>
                                <span class="value">{ &manifest_info.name }</span>
                            </div>
                            <div class="row">
                                <span class="label">{ "Date: " }</span>
                                <span class="value">{ &manifest_info.date }</span>
                            </div>
                            </>
                        }
                    } else {
                        html! { <p>{ "No manifest available" }</p> }
                    }
                }
                </div>
                <div class="actions">
                    <button onclick={on_fetch.clone()} class="btn btn-primary">{ "Check manifest updates" }</button>
                    <a href="/api/manifest/latest" download="manifest.json" class={ classes!("btn", "btn-primary", "no-underline", (!has_manifest).then_some("disabled"))}>{ "Download manifest" }</a>
                </div>
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct DownloadsListProps {
    pub downloads: Vec<DownloadItem>,
}

#[function_component(DownloadsList)]
pub fn downloads_list(DownloadsListProps { downloads }: &DownloadsListProps) -> Html {
    html! {
        <div class="status-section">
            <h2>{ "Pending Downloads" }</h2>
            if downloads.is_empty() {
                <p>{ "No pending downloads." }</p>
            } else {
                <div class="list downloads-list">
                {
                    for downloads.iter().map(|item| html! {
                        <div class="card download-card">
                             <div class="details">
                                <h3>{ &item.name }</h3>
                                <span class={match item.status {
                                    VideoStatus::Pending => "status-pending",
                                    VideoStatus::Downloading(_) => "status-downloading",
                                    VideoStatus::Failed(_) => "status-failed",
                                    VideoStatus::Downloaded => "status-downloaded",
                                }}>
                                    { match &item.status {
                                        VideoStatus::Pending => "Pending".to_string(),
                                        VideoStatus::Downloading(p) => format!("Downloading ({:.0}%)", p.0 * 100.0),
                                        VideoStatus::Failed(msg) => format!("Failed: {msg}"),
                                    VideoStatus::Downloaded => "Downloaded".to_string(),
                                    }}
                                </span>
                             </div>
                             if let VideoStatus::Downloading(progress) = &item.status {
                                <div class="progress-bar-container">
                                    <div class="progress-bar" style={format!("width: {:.0}%;", progress.0 * 100.0)}></div>
                                </div>
                             }
                        </div>
                    })
                }
                </div>
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct LogViewerProps {
    logs: Vec<LogEntry>,
}

#[function_component(LogViewer)]
pub fn log_viewer(LogViewerProps { logs }: &LogViewerProps) -> Html {
    html! {
        <div class="status-section">
            <h2>{ "System Logs" }</h2>
            <div class="logs-container">
                {
                    logs.iter().map(|log| html! {
                        <div class={classes!("log-entry", log.level.as_str().to_lowercase())}>
                            <span class="log-time">{ &log.timestamp }</span>
                            <span class="log-level">{ log.level.as_str() }</span>
                            <span class="log-message">{ &log.message }</span>
                            {
                                log.kv_pairs.iter().map(|(k, v)| {
                                    html! {
                                        <>
                                        <span class="log-key">{ k }</span>
                                        <span class="log-value">{ v }</span>
                                        </>
                                    }
                                }).collect::<Html>()
                            }
                        </div>
                    }).collect::<Html>()
                }
            </div>
            <div class="card details-card">
                <div class="actions">
                    <a href="/api/logfile" download="leap_logs.json" class="btn-primary no-underline">{ "Download logfile" }</a>
                </div>
            </div>
        </div>
    }
}

async fn fetch_version_info() -> anyhow::Result<BuildInfo> {
    let resp = Request::get("/api/version").send().await?;

    if !resp.ok() {
        anyhow::bail!("Response is not successful: {}", resp.status());
    }

    let text = resp.text().await?;
    Ok(serde_json::from_str(&text)?)
}

async fn fetch_logs() -> anyhow::Result<Vec<LogEntry>> {
    let mut new_logs = vec![];
    let resp = Request::get("/api/logfile").send().await?;

    if !resp.ok() {
        anyhow::bail!("Response is not successful: {}", resp.status());
    }

    let text = resp.text().await?;

    // The file might be very large, so take only the last X logs.
    // More would not be that useful either, so we just provide a way to
    // download them if needed
    const MAX_LOGS: usize = 200;
    for log in text.lines().rev().take(MAX_LOGS) {
        let log = serde_json::from_str(log)?;
        let log: LogEntry = log;
        new_logs.push(log);
    }
    new_logs.reverse();
    Ok(new_logs)
}

async fn fetch_manifest_info() -> anyhow::Result<Option<(String, ManifestInfo)>> {
    let resp = Request::get("/api/manifest/latest").send().await?;

    if !resp.ok() {
        anyhow::bail!("Response is not successful: {}", resp.status());
    }

    let text = resp.text().await?;
    if text.is_empty() {
        return Ok(None);
    }
    let info = serde_json::from_str(&text)?;
    Ok(Some((text, info)))
}

async fn trigger_manifest_update_check() -> anyhow::Result<()> {
    let resp = Request::post("/api/manifest/fetch").send().await?;
    if !resp.ok() {
        anyhow::bail!("Response is not successful: {}", resp.status());
    }
    Ok(())
}

#[derive(Properties, PartialEq)]
pub struct VersionInfoProps {
    pub version: BuildInfo,
}

#[function_component(VersionInfo)]
pub fn version_info(VersionInfoProps { version }: &VersionInfoProps) -> Html {
    html! {
        <div class="status-section">
            <h2>{ "Build Information" }</h2>
            <div class="card details-card">
                <div class="details">
                    <div class="row">
                        <span class="label">{ "Name: " }</span>
                        <span class="value">{ &version.name }</span>
                    </div>
                    <div class="row">
                        <span class="label">{ "Version: " }</span>
                        <span class="value">{ &version.version }</span>
                    </div>
                    if let Some(git_hash) = &version.git_hash {
                        <div class="row">
                            <span class="label">{ "Git Hash: " }</span>
                            <span class="value">{ git_hash }</span>
                        </div>
                    }
                    <div class="row">
                        <span class="label">{ "Authors: " }</span>
                        <span class="value">{ for version.authors.iter().map(|author| html! { <span>{ author }</span> }) }</span>
                    </div>
                    <div class="row">
                        <span class="label">{ "Homepage: " }</span>
                        <span class="value"><a href={ version.homepage.clone() } target="_blank">{ &version.homepage }</a></span>
                    </div>
                    <div class="row">
                        <span class="label">{ "License: " }</span>
                        <span class="value">{ &version.license }</span>
                    </div>
                    <div class="row">
                        <span class="label">{ "Repository: " }</span>
                        <span class="value"><a href={ version.repository.clone() } target="_blank">{ &version.repository }</a></span>
                    </div>
                    <div class="row">
                        <span class="label">{ "Profile: " }</span>
                        <span class="value">{ &version.profile }</span>
                    </div>
                    <div class="row">
                        <span class="label">{ "Rustc: " }</span>
                        <span class="value">{ &version.rustc_version }</span>
                    </div>
                     <div class="row">
                        <span class="label">{ "Features: " }</span>
                        <span class="value">{ &version.features }</span>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[function_component(StatusDashboard)]
pub fn status_dashboard() -> Html {
    let state_data = use_state(|| None);

    let context = use_context::<ContentContextHandle>().expect("ContentContext not found");
    let sections_loaded = context.sections.is_some();

    use_effect_with(sections_loaded, {
        let context = context.clone();
        let state_data = state_data.clone();
        move |_| {
            spawn_local(async move {
                if let Some(sections) = context.sections.as_ref()
                    && state_data.is_none()
                {
                    let version = match fetch_version_info().await {
                        Ok(logs) => logs,
                        Err(e) => {
                            web_sys::console::log_1(
                                &format!("Error while fetching LEAP version: {e}").into(),
                            );
                            return;
                        }
                    };

                    let logs = match fetch_logs().await {
                        Ok(logs) => logs,
                        Err(e) => {
                            web_sys::console::log_1(
                                &format!("Error while fetching LEAP logs: {e}").into(),
                            );
                            return;
                        }
                    };

                    let manifest = match fetch_manifest_info().await {
                        Ok(v) => v,
                        Err(e) => {
                            web_sys::console::log_1(
                                &format!("Error while fetching manifest information: {e}").into(),
                            );
                            return;
                        }
                    };

                    let pending_downloads = sections
                        .iter()
                        .flat_map(|s| &s.content)
                        .filter(|v| v.status != VideoStatus::Downloaded)
                        .map(|v| DownloadItem {
                            name: v.name.clone(),
                            id: v.id.clone(),
                            status: v.status.clone(),
                        })
                        .collect();

                    state_data.set(Some(Status {
                        version,
                        logs,
                        manifest,
                        pending_downloads,
                    }));
                }
            });
        }
    });

    let on_fetch = Callback::from(|_| {
        web_sys::console::log_1(&"Triggering manifest fetch...".into());
        spawn_local(async {
            let _ = trigger_manifest_update_check().await.inspect_err(|e| {
                web_sys::console::log_1(&format!("Failed to request manifest fetch: {e}").into());
            });
        });
    });

    html! {
        <div class="page status-page">
            <header class="header">
                <h1>{ "System Status" }</h1>
            </header>

            <div class="status-content">
                {
                    if let Some(state_data) = &*state_data {
                        html! {
                            <>
                                <ManifestStatus manifest={state_data.manifest.clone()} on_fetch={on_fetch} />
                                <DownloadsList downloads={state_data.pending_downloads.clone()} />
                                <VersionInfo version={state_data.version.clone()} />
                                <LogViewer logs={state_data.logs.clone()} />
                            </>
                        }
                    } else {
                        html! {
                            <p>{ "Loading..." }</p>
                        }
                    }
                }
            </div>
        </div>
    }
}
