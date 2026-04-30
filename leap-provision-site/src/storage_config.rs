use crate::app::{Route, use_provision_redirect};
use gloo_net::http::Request;
use leap_api::provision::storage::devices::get::BlockDevice;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

fn format_size(bytes: u64) -> String {
    const GIB: u64 = 1 << 30;
    const MIB: u64 = 1 << 20;
    const KIB: u64 = 1 << 10;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

async fn fetch_devices() -> Option<Vec<BlockDevice>> {
    let response = match Request::get("/provision/storage/devices").send().await {
        Ok(r) => r,
        Err(e) => {
            log::error!("Failed to fetch storage devices: {e:?}");
            return None;
        }
    };
    match response.json::<Vec<BlockDevice>>().await {
        Ok(devices) => Some(devices),
        Err(e) => {
            log::error!("Failed to parse storage devices: {e:?}");
            None
        }
    }
}

#[function_component(StorageConfigPage)]
pub fn storage_config_page() -> Html {
    use_provision_redirect(Route::StorageConfig);

    let devices: UseStateHandle<Option<Vec<BlockDevice>>> = use_state(|| None);
    let selected = use_state(String::new);
    let refresh = use_state(|| 0u32);
    let formatting = use_state(|| false);
    let toast: UseStateHandle<Option<String>> = use_state(|| None);

    {
        let devices = devices.clone();
        let selected = selected.clone();
        use_effect_with(*refresh, move |_| {
            devices.set(None);
            spawn_local(async move {
                if let Some(fetched) = fetch_devices().await {
                    if let Some(first) = fetched.first() {
                        selected.set(first.name.clone());
                    }
                    devices.set(Some(fetched));
                }
            });
            || ()
        });
    }

    let on_device_change = {
        let selected = selected.clone();
        Callback::from(move |e: Event| {
            use web_sys::HtmlSelectElement;
            selected.set(e.target_unchecked_into::<HtmlSelectElement>().value());
        })
    };

    let on_refresh = {
        let refresh = refresh.clone();
        Callback::from(move |_| refresh.set(*refresh + 1))
    };

    let on_dismiss_toast = {
        let toast = toast.clone();
        Callback::from(move |_| toast.set(None))
    };

    let navigator = use_navigator().unwrap();
    let on_format = {
        let selected = selected.clone();
        let formatting = formatting.clone();
        let toast = toast.clone();

        Callback::from(move |_| {
            let selected_val = (*selected).clone();
            let formatting = formatting.clone();
            let toast = toast.clone();
            let navigator = navigator.clone();

            let confirmed = web_sys::window()
                .and_then(|w| {
                    w.confirm_with_message(&format!(
                        "This will erase all data on {selected_val}. Are you sure?"
                    ))
                    .ok()
                })
                .unwrap_or(false);

            if !confirmed {
                return;
            }

            formatting.set(true);

            spawn_local(async move {
                let url = format!("/provision/storage/format?name={selected_val}");
                let response = match Request::post(&url).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        toast.set(Some(format!("Request failed: {e}")));
                        formatting.set(false);
                        return;
                    }
                };

                if !response.ok() {
                    let body = response.text().await.unwrap_or_default();
                    toast.set(Some(if body.is_empty() {
                        format!("Format failed ({})", response.status())
                    } else {
                        body
                    }));
                    formatting.set(false);
                    return;
                }

                if let Ok(status_resp) = Request::get("/provision/status").send().await
                    && let Ok(status) = status_resp.json::<leap_api::types::ProvisionStatus>().await
                {
                    navigator.replace(&Route::from(status));
                }

                formatting.set(false);
            });
        })
    };

    html! {
        <div class="page">
            <h1>{ "Storage Configuration" }</h1>
            if let Some(msg) = (*toast).clone() {
                <div class="toast toast-error">
                    <span>{ msg }</span>
                    <button onclick={on_dismiss_toast}>{ "✕" }</button>
                </div>
            }
            <div class="form">
                <div class="form-field">
                    <label for="storage-device">{ "Storage device" }</label>
                    {
                        match &*devices {
                            None => html! {
                                <select id="storage-device" disabled=true>
                                    <option>{ "Loading…" }</option>
                                </select>
                            },
                            Some(devs) => {
                                html! {
                                    <select id="storage-device" onchange={on_device_change}>
                                        { for devs.iter().map(|dev| html! {
                                            <option value={dev.name.clone()}
                                                selected={dev.name == *selected}>
                                                { format!("{} — {} ({})", dev.name.clone(), dev.device_type, format_size(dev.size)) }
                                            </option>
                                        }) }
                                    </select>
                                }
                            }
                        }
                    }
                    <button onclick={on_refresh}>{ "Refresh" }</button>
                </div>
                <div class="form-actions">
                    <button class="btn-primary" onclick={on_format} disabled={*formatting}>
                        if *formatting { { "Please wait…" } } else { { "Format" } }
                    </button>
                </div>
                if *formatting {
                    <p class="reconnect-notice">
                        { "Formatting storage device, please wait…" }
                    </p>
                }
            </div>
        </div>
    }
}
