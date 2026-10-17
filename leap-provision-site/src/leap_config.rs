//! Component for configuring LEAP-specific settings.
//!
//! This component provides a form to configure:
//! - **Downloader settings**: Concurrent downloads, update intervals, and retry backoff parameters.
//! - **S3 settings**: Bucket URI, access keys, endpoint URL, region, and path-style access.
//!
//! When configured, the settings are sent to the server at `/provision/config`.
//! The component handles the asynchronous submission and manages the connection
//! state during the device's reconfiguration and potential network switch.

use crate::app::{Route, use_provision_redirect};
use gloo_net::http::Request;
use gloo_timers::future::sleep;
use leap_api::types::{DownloaderConfig, LeapConfig, ProvisionStatus, RetryParams, S3Config};
use secrecy::SecretString;
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;
use web_sys::{FormData, HtmlFormElement};
use yew::prelude::*;
use yew_router::prelude::*;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
enum LeapConfigError {
    #[error("Concurrent downloads must be a positive integer")]
    ConcurrentDownloads,
    #[error("Update interval must be a duration such as 1h or 30m")]
    UpdateInterval,
    #[error("Initial retry backoff must be a duration such as 1s or 5m")]
    InitialBackoff,
    #[error("Backoff factor must be a number greater than 1")]
    BackoffFactor,
    #[error("Maximum retry backoff must be a duration such as 1h or 30m")]
    MaxBackoff,
    #[error("Bucket must be a valid URI such as s3://my-bucket")]
    Bucket,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum LeapConfigState {
    Idle,
    Submitting,
    Reconnecting,
}

const FORM_ITEM_CONCURRENT_DOWNLOADS: &str = "concurrent-downloads";
const FORM_ITEM_UPDATE_INTERVAL: &str = "update-interval";
const FORM_ITEM_INITIAL_BACKOFF: &str = "initial-backoff";
const FORM_ITEM_BACKOFF_FACTOR: &str = "backoff-factor";
const FORM_ITEM_MAX_BACKOFF: &str = "max-backoff";
const FORM_ITEM_BUCKET: &str = "bucket";
const FORM_ITEM_ACCESS_KEY_ID: &str = "access-key-id";
const FORM_ITEM_SECRET_ACCESS_KEY: &str = "secret-access-key";
const FORM_ITEM_ENDPOINT_URL: &str = "endpoint-url";
const FORM_ITEM_REGION: &str = "region";
const FORM_ITEM_FORCE_PATH_STYLE: &str = "force-path-style";

/// Builds the [`LeapConfig`] to submit from the values currently held by the form.
fn leap_config_from_form(form: HtmlFormElement) -> Result<LeapConfig, LeapConfigError> {
    let form = FormData::new_with_form(&form).unwrap();
    let extract_form_value = |name| form.get(name).as_string().unwrap_or_default();
    let extract_optional_form_value =
        |name| Some(extract_form_value(name)).filter(|v| !v.is_empty());
    let extract_duration = |name: &'static str, error: LeapConfigError| {
        humantime::parse_duration(&extract_form_value(name)).map_err(|_| error)
    };

    let concurrent_downloads = extract_form_value(FORM_ITEM_CONCURRENT_DOWNLOADS)
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or(LeapConfigError::ConcurrentDownloads)?;
    let update_interval =
        extract_duration(FORM_ITEM_UPDATE_INTERVAL, LeapConfigError::UpdateInterval)?;
    let initial_backoff =
        extract_duration(FORM_ITEM_INITIAL_BACKOFF, LeapConfigError::InitialBackoff)?;
    let backoff_factor = extract_form_value(FORM_ITEM_BACKOFF_FACTOR)
        .parse::<f64>()
        .ok()
        .filter(|f| *f > 1.0)
        .ok_or(LeapConfigError::BackoffFactor)?;
    let max_backoff = extract_duration(FORM_ITEM_MAX_BACKOFF, LeapConfigError::MaxBackoff)?;
    let bucket = extract_form_value(FORM_ITEM_BUCKET)
        .parse()
        .map_err(|_| LeapConfigError::Bucket)?;

    Ok(LeapConfig {
        downloader_config: DownloaderConfig {
            concurrent_downloads,
            update_interval,
            retry_params: RetryParams {
                initial_backoff,
                backoff_factor,
                max_backoff,
            },
        },
        s3_config: S3Config {
            bucket,
            access_key_id: SecretString::from(extract_form_value(FORM_ITEM_ACCESS_KEY_ID)),
            secret_access_key: SecretString::from(extract_form_value(FORM_ITEM_SECRET_ACCESS_KEY)),
            endpoint_url: extract_optional_form_value(FORM_ITEM_ENDPOINT_URL),
            // An unchecked checkbox is left out of the form data entirely.
            force_path_style: Some(form.has(FORM_ITEM_FORCE_PATH_STYLE)),
            region: extract_optional_form_value(FORM_ITEM_REGION),
        },
    })
}

/// Sends the LEAP configuration to the server and follows the provisioning
/// status until the device has either applied or rejected it.
fn submit_leap_configuration(
    config: LeapConfig,
    state: UseStateHandle<LeapConfigState>,
    toast: UseStateHandle<Option<String>>,
    navigator: Navigator,
) {
    state.set(LeapConfigState::Submitting);

    // TODO: Update how the request is handled today, which is brittle and attempts to infer
    // that the device probably disconnected, but there's no guarantee that the device even
    // got the request.
    // The API between the frontend and the backend needs to be updated for this.
    spawn_local(async move {
        let request = match Request::post("/provision/config").json(&config) {
            Ok(r) => r,
            Err(e) => {
                toast.set(Some(format!("Failed to serialize request: {e}")));
                state.set(LeapConfigState::Idle);
                return;
            }
        };

        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => {
                // Connection dropped. The device is likely switching networks to test
                // the S3 configuration (includes NTP sync, which may take extra time).
                log::warn!("POST /provision/config failed, waiting for the device: {e}");
                state.set(LeapConfigState::Reconnecting);

                // 75 polls × 2 s = 150 s, covering 30 s network activate + 30 s NTP
                // sync + S3 connectivity check + reconnect buffer.
                let mut result_msg: Option<String> = None;
                let mut navigated = false;
                for _ in 0..75 {
                    sleep(Duration::from_secs(2)).await;

                    let Ok(resp) = Request::get("/provision/status").send().await else {
                        log::warn!("GET /provision/status failed");
                        continue;
                    };

                    let Ok(status) = resp.json::<ProvisionStatus>().await else {
                        log::warn!(
                            "GET /provision/status could not be read as `ProvisionStatus` JSON"
                        );
                        continue;
                    };

                    if status == ProvisionStatus::LeapConfig {
                        result_msg = Some(
                            "LEAP configuration could not be applied. \
                                Please check your S3 settings and try again."
                                .to_string(),
                        );
                    } else {
                        log::info!("LEAP configuration was successfully applied.");
                        navigator.replace(&Route::from(status));
                        navigated = true;
                    }
                    break;
                }

                if !navigated {
                    let result_msg = result_msg.unwrap_or_else(|| {
                        "The device did not reconnect within the expected time. \
                         Please verify the network and S3 settings and try again."
                            .to_string()
                    });
                    log::error!("Failed to apply LEAP config: {result_msg}");
                    toast.set(Some(result_msg));
                }

                state.set(LeapConfigState::Idle);
                return;
            }
        };

        if !response.ok() {
            let body = response.text().await.unwrap_or_default();
            log::warn!("LEAP configuration failed: ({response:?}): {body}");
            toast.set(Some(if body.is_empty() {
                format!("Configuration failed ({})", response.status())
            } else {
                body
            }));
            state.set(LeapConfigState::Idle);
            return;
        }

        if let Ok(status_resp) = Request::get("/provision/status").send().await
            && let Ok(status) = status_resp.json::<ProvisionStatus>().await
        {
            log::info!("LEAP configuration was successfully applied.");
            navigator.replace(&Route::from(status));
        }

        state.set(LeapConfigState::Idle);
    });
}

fn on_dismiss_toast_callback(toast: &UseStateHandle<Option<String>>) -> Callback<MouseEvent> {
    let toast = toast.clone();
    Callback::from(move |_| toast.set(None))
}

fn on_configure_callback(
    toast: UseStateHandle<Option<String>>,
    state: UseStateHandle<LeapConfigState>,
    navigator: Navigator,
) -> Callback<SubmitEvent> {
    Callback::from(move |e: SubmitEvent| {
        e.prevent_default();

        let config = match leap_config_from_form(e.target_unchecked_into()) {
            Ok(cfg) => cfg,
            Err(err) => {
                toast.set(Some(format!("{err}")));
                state.set(LeapConfigState::Idle);
                return;
            }
        };
        submit_leap_configuration(config, state.clone(), toast.clone(), navigator.clone());
    })
}

/// The component for configuring LEAP-specific settings.
#[function_component(LeapConfigPage)]
pub fn leap_config_page() -> Html {
    use_provision_redirect(Route::LeapConfig);

    let toast: UseStateHandle<Option<String>> = use_state(|| None);
    let state = use_state(|| LeapConfigState::Idle);
    let navigator = use_navigator().unwrap();

    let on_dismiss_toast = on_dismiss_toast_callback(&toast);
    let on_configure = on_configure_callback(toast.clone(), state.clone(), navigator);

    // The inputs are uncontrolled: defaults are set through the `defaultValue`
    // property, since `value` would be re-applied on every render and discard
    // whatever the user typed.
    html! {
        <div class="page leap-config-page">
            <h1>{ "LEAP Configuration" }</h1>
            if let Some(msg) = (*toast).clone() {
                <div class="toast toast-error">
                    <span>{ msg }</span>
                    <button onclick={on_dismiss_toast}>{ "✕" }</button>
                </div>
            }
            <form onsubmit={on_configure} class="form">
                <h2>{ "Downloader" }</h2>

                <div class="form-field">
                    <label for={FORM_ITEM_CONCURRENT_DOWNLOADS}>{ "Concurrent downloads" }</label>
                    <input id={FORM_ITEM_CONCURRENT_DOWNLOADS} name={FORM_ITEM_CONCURRENT_DOWNLOADS}
                        type="number" min="1" step="1" required=true ~defaultValue="4" />
                </div>
                <div class="form-field">
                    <label for={FORM_ITEM_UPDATE_INTERVAL}>{ "Update interval" }</label>
                    <input id={FORM_ITEM_UPDATE_INTERVAL} name={FORM_ITEM_UPDATE_INTERVAL}
                        type="text" placeholder="1h" required=true ~defaultValue="1h" />
                </div>
                <div class="form-field">
                    <label for={FORM_ITEM_INITIAL_BACKOFF}>{ "Initial retry backoff" }</label>
                    <input id={FORM_ITEM_INITIAL_BACKOFF} name={FORM_ITEM_INITIAL_BACKOFF}
                        type="text" placeholder="1s" required=true ~defaultValue="1s" />
                </div>
                <div class="form-field">
                    <label for={FORM_ITEM_BACKOFF_FACTOR}>{ "Backoff factor" }</label>
                    <input id={FORM_ITEM_BACKOFF_FACTOR} name={FORM_ITEM_BACKOFF_FACTOR}
                        type="number" min="1.01" step="any" required=true ~defaultValue="2" />
                </div>
                <div class="form-field">
                    <label for={FORM_ITEM_MAX_BACKOFF}>{ "Maximum retry backoff" }</label>
                    <input id={FORM_ITEM_MAX_BACKOFF} name={FORM_ITEM_MAX_BACKOFF}
                        type="text" placeholder="1h" required=true ~defaultValue="1h" />
                </div>

                <h2>{ "S3 Storage" }</h2>

                <div class="form-field">
                    <label for={FORM_ITEM_BUCKET}>{ "Bucket URI" }</label>
                    <input id={FORM_ITEM_BUCKET} name={FORM_ITEM_BUCKET}
                        type="text" placeholder="s3://my-bucket" required=true />
                </div>
                <div class="form-field">
                    <label for={FORM_ITEM_ACCESS_KEY_ID}>{ "Access key ID" }</label>
                    <input id={FORM_ITEM_ACCESS_KEY_ID} name={FORM_ITEM_ACCESS_KEY_ID}
                        type="text" required=true />
                </div>
                <div class="form-field">
                    <label for={FORM_ITEM_SECRET_ACCESS_KEY}>{ "Secret access key" }</label>
                    <input id={FORM_ITEM_SECRET_ACCESS_KEY} name={FORM_ITEM_SECRET_ACCESS_KEY}
                        type="password" required=true />
                </div>
                <div class="form-field">
                    <label for={FORM_ITEM_ENDPOINT_URL}>{ "Endpoint URL" }</label>
                    <input id={FORM_ITEM_ENDPOINT_URL} name={FORM_ITEM_ENDPOINT_URL}
                        type="text" placeholder="https://... (optional)" />
                </div>
                <div class="form-field">
                    <label for={FORM_ITEM_REGION}>{ "Region" }</label>
                    <input id={FORM_ITEM_REGION} name={FORM_ITEM_REGION}
                        type="text" placeholder="us-east-1 (optional)" />
                </div>
                <div class="form-field">
                    <label class="checkbox-label">
                        <input id={FORM_ITEM_FORCE_PATH_STYLE} name={FORM_ITEM_FORCE_PATH_STYLE}
                            type="checkbox" />
                        { "Force path-style bucket access" }
                    </label>
                </div>

                <div class="form-actions">
                    <button type="submit" class="btn-primary" disabled={*state != LeapConfigState::Idle}>
                        if *state != LeapConfigState::Idle { { "Please wait…" } } else { { "Configure" } }
                    </button>
                </div>
                if *state == LeapConfigState::Reconnecting {
                    <p class="reconnect-notice">
                        { "Waiting for the device to reconnect and verify the S3 \
                           configuration. This may take a couple of minutes…" }
                    </p>
                }
            </form>
        </div>
    }
}
