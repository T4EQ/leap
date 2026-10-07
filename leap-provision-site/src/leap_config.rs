//! Component for configuring LEAP-specific settings.
//!
//! This component provides a form to configure:
//! - **Downloader settings**: Concurrent downloads, update intervals, and retry backoff parameters.
//! - **S3 settings**: Bucket URI, access keys, endpoint URL, region, and path-style access.
//!
//! When configured, the settings are sent to the server at `/provision/config`.
//! The component handles the asynchronous submission and manages the connection
//! state during the device's reconfiguration and potential network switch.

use crate::{
    app::{Route, use_provision_redirect},
    oninput,
};
use gloo_net::http::Request;
use gloo_timers::future::sleep;
use leap_api::types::ProvisionStatus;
use secrecy::ExposeSecret;
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

/// The component for configuring LEAP-specific settings.
#[function_component(LeapConfigPage)]
pub fn leap_config_page() -> Html {
    use_provision_redirect(Route::LeapConfig);

    // Downloader fields
    let concurrent_downloads = use_state(|| "4".to_string());
    let update_interval = use_state(|| "1h".to_string());
    let initial_backoff = use_state(|| "1s".to_string());
    let backoff_factor = use_state(|| "2".to_string());
    let max_backoff = use_state(|| "1h".to_string());

    // S3 fields
    let bucket = use_state(String::new);
    let access_key_id = use_state(String::new);
    let secret_access_key = use_state(String::new);
    let endpoint_url = use_state(String::new);
    let force_path_style = use_state(|| false);
    let region = use_state(String::new);

    let toast: UseStateHandle<Option<String>> = use_state(|| None);
    let submitting = use_state(|| false);
    let reconnecting = use_state(|| false);
    let imported = use_state(|| false);
    let advanced_open = use_state(|| false);
    let on_advanced_toggle = {
        let advanced_open = advanced_open.clone();
        Callback::from(move |event: Event| {
            let element = event.target_unchecked_into::<web_sys::Element>();
            advanced_open.set(element.has_attribute("open"));
        })
    };

    let onimport = {
        let imported = imported.clone();
        let concurrent_downloads = concurrent_downloads.clone();
        let update_interval = update_interval.clone();
        let initial_backoff = initial_backoff.clone();
        let backoff_factor = backoff_factor.clone();
        let max_backoff = max_backoff.clone();
        let bucket = bucket.clone();
        let access_key_id = access_key_id.clone();
        let secret_access_key = secret_access_key.clone();
        let endpoint_url = endpoint_url.clone();
        let force_path_style = force_path_style.clone();
        let region = region.clone();
        Callback::from(move |config: leap_api::types::LeapConfig| {
            let d = config.downloader_config;
            concurrent_downloads.set(d.concurrent_downloads.to_string());
            update_interval.set(format!("{}s", d.update_interval.as_secs_f64()));
            initial_backoff.set(format!("{}s", d.retry_params.initial_backoff.as_secs_f64()));
            backoff_factor.set(d.retry_params.backoff_factor.to_string());
            max_backoff.set(format!("{}s", d.retry_params.max_backoff.as_secs_f64()));
            let s = config.s3_config;
            bucket.set(s.bucket.to_string());
            access_key_id.set(s.access_key_id.expose_secret().to_string());
            secret_access_key.set(s.secret_access_key.expose_secret().to_string());
            endpoint_url.set(s.endpoint_url.unwrap_or_default());
            force_path_style.set(s.force_path_style.unwrap_or(false));
            region.set(s.region.unwrap_or_default());
            imported.set(true);
        })
    };

    let on_force_path_style_change = {
        let force_path_style = force_path_style.clone();
        Callback::from(move |e: Event| {
            use web_sys::HtmlInputElement;
            force_path_style.set(e.target_unchecked_into::<HtmlInputElement>().checked());
        })
    };

    let on_dismiss_toast = {
        let toast = toast.clone();
        Callback::from(move |_| toast.set(None))
    };

    let navigator = use_navigator().unwrap();
    let on_configure = {
        let concurrent_downloads = concurrent_downloads.clone();
        let update_interval = update_interval.clone();
        let initial_backoff = initial_backoff.clone();
        let backoff_factor = backoff_factor.clone();
        let max_backoff = max_backoff.clone();
        let bucket = bucket.clone();
        let access_key_id = access_key_id.clone();
        let secret_access_key = secret_access_key.clone();
        let endpoint_url = endpoint_url.clone();
        let force_path_style = force_path_style.clone();
        let region = region.clone();
        let toast = toast.clone();
        let submitting = submitting.clone();
        let reconnecting = reconnecting.clone();

        Callback::from(move |_| {
            let navigator = navigator.clone();
            let concurrent_downloads_val = (*concurrent_downloads).clone();
            let update_interval_val = (*update_interval).clone();
            let initial_backoff_val = (*initial_backoff).clone();
            let backoff_factor_val = (*backoff_factor).clone();
            let max_backoff_val = (*max_backoff).clone();
            let bucket_val = (*bucket).clone();
            let access_key_id_val = (*access_key_id).clone();
            let secret_access_key_val = (*secret_access_key).clone();
            let endpoint_url_val = (*endpoint_url).clone();
            let force_path_style_val = *force_path_style;
            let region_val = (*region).clone();
            let toast = toast.clone();
            let submitting = submitting.clone();
            let reconnecting = reconnecting.clone();

            let concurrent_downloads_num = match concurrent_downloads_val.parse::<usize>() {
                Ok(n) if n > 0 => n,
                _ => {
                    toast.set(Some(
                        "Concurrent downloads must be a positive integer".to_string(),
                    ));
                    return;
                }
            };

            let backoff_factor_num = match backoff_factor_val.parse::<f64>() {
                Ok(f) if f > 1.0 => f,
                _ => {
                    toast.set(Some(
                        "Backoff factor must be a number greater than 1".to_string(),
                    ));
                    return;
                }
            };

            if bucket_val.is_empty() {
                toast.set(Some("Bucket URI is required".to_string()));
                return;
            }
            if access_key_id_val.is_empty() {
                toast.set(Some("Access key ID is required".to_string()));
                return;
            }
            if secret_access_key_val.is_empty() {
                toast.set(Some("Secret access key is required".to_string()));
                return;
            }

            let endpoint_url_opt: Option<String> = if endpoint_url_val.is_empty() {
                None
            } else {
                Some(endpoint_url_val)
            };

            let region_opt: Option<String> = if region_val.is_empty() {
                None
            } else {
                Some(region_val)
            };

            let config = serde_json::json!({
                "downloader_config": {
                    "concurrent_downloads": concurrent_downloads_num,
                    "update_interval": update_interval_val,
                    "retry_params": {
                        "initial_backoff": initial_backoff_val,
                        "backoff_factor": backoff_factor_num,
                        "max_backoff": max_backoff_val,
                    }
                },
                "s3_config": {
                    "bucket": bucket_val,
                    "access_key_id": access_key_id_val,
                    "secret_access_key": secret_access_key_val,
                    "endpoint_url": endpoint_url_opt,
                    "force_path_style": Some(force_path_style_val),
                    "region": region_opt,
                }
            });

            submitting.set(true);

            spawn_local(async move {
                let request = match Request::post("/provision/config").json(&config) {
                    Ok(r) => r,
                    Err(e) => {
                        toast.set(Some(format!("Failed to serialize request: {e}")));
                        submitting.set(false);
                        return;
                    }
                };

                let response = match request.send().await {
                    Ok(r) => r,
                    Err(_) => {
                        // Connection dropped — the device is likely switching networks to test
                        // the S3 configuration (includes NTP sync, which may take extra time).
                        reconnecting.set(true);

                        // 75 polls × 2 s = 150 s, covering 30 s network activate + 30 s NTP
                        // sync + S3 connectivity check + reconnect buffer.
                        let mut result_msg: Option<String> = None;
                        let mut navigated = false;
                        for _ in 0..75u32 {
                            sleep(Duration::from_secs(2)).await;
                            let Ok(resp) = Request::get("/provision/status").send().await else {
                                continue;
                            };
                            let Ok(status) = resp.json::<ProvisionStatus>().await else {
                                continue;
                            };
                            if status == ProvisionStatus::LeapConfig {
                                result_msg = Some(
                                    "LEAP configuration could not be applied. \
                                        Please check your S3 settings and try again."
                                        .to_string(),
                                );
                            } else {
                                navigator.replace(&Route::from(status));
                                navigated = true;
                            }
                        }

                        if !navigated {
                            toast.set(Some(result_msg.unwrap_or_else(|| {
                                "The device did not reconnect within the expected time. \
                                 Please verify the network and S3 settings and try again."
                                    .to_string()
                            })));
                        }

                        reconnecting.set(false);
                        submitting.set(false);
                        return;
                    }
                };

                if !response.ok() {
                    let body = response.text().await.unwrap_or_default();
                    toast.set(Some(if body.is_empty() {
                        format!("Configuration failed ({})", response.status())
                    } else {
                        body
                    }));
                    submitting.set(false);
                    return;
                }

                if let Ok(status_resp) = Request::get("/provision/status").send().await
                    && let Ok(status) = status_resp.json::<ProvisionStatus>().await
                {
                    navigator.replace(&Route::from(status));
                }

                submitting.set(false);
            });
        })
    };

    html! {
        <div class="page leap-config-page">
            <h1>{ "Set up your content" }</h1>
            <p class="setup-intro">{"Use the QR code your administrator gave you."}</p>
            if let Some(msg) = (*toast).clone() {
                <div class="toast toast-error">
                    <span>{ msg }</span>
                    <button onclick={on_dismiss_toast}>{ "✕" }</button>
                </div>
            }
            <crate::import_controls::ImportControls {onimport} disabled={*submitting} />
            if *imported {
                <div class="settings-ready" role="status"><span aria-hidden="true">{"✓"}</span>{"Your settings are ready."}</div>
            }
            if *imported || *advanced_open {
                <button class="btn-primary apply-settings" onclick={on_configure} disabled={*submitting}>
                    if *submitting { {"Applying settings…"} } else { {"Apply settings"} }
                </button>
            }
            if *reconnecting {
                <p class="reconnect-notice">{"Waiting for the device to reconnect and check your settings. This may take a couple of minutes…"}</p>
            }
            <p class="qr-privacy">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
                    <rect x="5" y="10" width="14" height="11" rx="2" /><path d="M8 10V6a4 4 0 0 1 8 0v4" />
                </svg>
                {"Keep your QR code private."}
            </p>
            <details class="advanced-settings" ontoggle={on_advanced_toggle}>
                <summary>{"Advanced: enter settings manually"}</summary>
                <fieldset class="form" disabled={*submitting}>
                <h2>{ "Downloader" }</h2>

                <div class="form-field">
                    <label for="concurrent-downloads">{ "Concurrent downloads" }</label>
                    <input id="concurrent-downloads" type="number" min="1"
                        value={(*concurrent_downloads).clone()}
                        oninput={oninput!(concurrent_downloads)} />
                </div>
                <div class="form-field">
                    <label for="update-interval">{ "Update interval" }</label>
                    <input id="update-interval" type="text" placeholder="1h"
                        value={(*update_interval).clone()}
                        oninput={oninput!(update_interval)} />
                </div>
                <div class="form-field">
                    <label for="initial-backoff">{ "Initial retry backoff" }</label>
                    <input id="initial-backoff" type="text" placeholder="1s"
                        value={(*initial_backoff).clone()}
                        oninput={oninput!(initial_backoff)} />
                </div>
                <div class="form-field">
                    <label for="backoff-factor">{ "Backoff factor" }</label>
                    <input id="backoff-factor" type="number" min="1.01" step="0.1"
                        value={(*backoff_factor).clone()}
                        oninput={oninput!(backoff_factor)} />
                </div>
                <div class="form-field">
                    <label for="max-backoff">{ "Maximum retry backoff" }</label>
                    <input id="max-backoff" type="text" placeholder="1h"
                        value={(*max_backoff).clone()}
                        oninput={oninput!(max_backoff)} />
                </div>

                <h2>{ "S3 Storage" }</h2>

                <div class="form-field">
                    <label for="bucket">{ "Bucket URI" }</label>
                    <input id="bucket" type="text" placeholder="s3://my-bucket"
                        value={(*bucket).clone()}
                        oninput={oninput!(bucket)} />
                </div>
                <div class="form-field">
                    <label for="access-key-id">{ "Access key ID" }</label>
                    <input id="access-key-id" type="text"
                        value={(*access_key_id).clone()}
                        oninput={oninput!(access_key_id)} />
                </div>
                <div class="form-field">
                    <label for="secret-access-key">{ "Secret access key" }</label>
                    <input id="secret-access-key" type="password"
                        value={(*secret_access_key).clone()}
                        oninput={oninput!(secret_access_key)} />
                </div>
                <div class="form-field">
                    <label for="endpoint-url">{ "Endpoint URL" }</label>
                    <input id="endpoint-url" type="text" placeholder="https://... (optional)"
                        value={(*endpoint_url).clone()}
                        oninput={oninput!(endpoint_url)} />
                </div>
                <div class="form-field">
                    <label for="region">{ "Region" }</label>
                    <input id="region" type="text" placeholder="us-east-1 (optional)"
                        value={(*region).clone()}
                        oninput={oninput!(region)} />
                </div>
                <div class="form-field">
                    <label class="checkbox-label">
                        <input id="force-path-style" type="checkbox"
                            checked={*force_path_style}
                            onchange={on_force_path_style_change} />
                        { "Force path-style bucket access" }
                    </label>
                </div>

                </fieldset>
            </details>
        </div>
    }
}
