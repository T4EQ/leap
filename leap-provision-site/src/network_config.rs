use crate::{
    app::{Route, use_provision_redirect},
    oninput,
};
use gloo_net::http::Request;
use gloo_timers::future::sleep;
use leap_api::types::{IpConfig, NetworkConfig, ProvisionStatus, StaticIpConfig, WiredConfig, WirelessConfig};
use secrecy::SecretString;
use std::net::Ipv4Addr;
use std::str::FromStr;
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;
use yew_router::prelude::*;

#[derive(Copy, Clone, PartialEq)]
enum ConnectionType {
    Wired,
    Wireless,
}

#[derive(Copy, Clone, PartialEq)]
enum IpMode {
    Dhcp,
    Static,
}

#[function_component(NetworkConfigPage)]
pub fn network_config_page() -> Html {
    use_provision_redirect(Route::NetworkConfig);

    let connection_type = use_state(|| ConnectionType::Wired);
    let ip_mode = use_state(|| IpMode::Dhcp);
    let ssid = use_state(String::new);
    let password = use_state(String::new);
    let ip_address = use_state(String::new);
    let gateway = use_state(String::new);
    let net_mask = use_state(String::new);
    let toast: UseStateHandle<Option<String>> = use_state(|| None);
    let submitting = use_state(|| false);
    let reconnecting = use_state(|| false);

    let on_connection_change = {
        let connection_type = connection_type.clone();
        Callback::from(move |e: Event| {
            let select = e.target_unchecked_into::<HtmlSelectElement>();
            connection_type.set(match select.value().as_str() {
                "wireless" => ConnectionType::Wireless,
                _ => ConnectionType::Wired,
            });
        })
    };

    let on_ip_mode_change = {
        let ip_mode = ip_mode.clone();
        Callback::from(move |e: Event| {
            let input = e.target_unchecked_into::<HtmlInputElement>();
            ip_mode.set(match input.value().as_str() {
                "static" => IpMode::Static,
                _ => IpMode::Dhcp,
            });
        })
    };

    let on_dismiss_toast = {
        let toast = toast.clone();
        Callback::from(move |_| toast.set(None))
    };

    let navigator = use_navigator().unwrap();
    let on_configure = {
        let connection_type = connection_type.clone();
        let ip_mode = ip_mode.clone();
        let ssid = ssid.clone();
        let password = password.clone();
        let ip_address = ip_address.clone();
        let gateway = gateway.clone();
        let net_mask = net_mask.clone();
        let toast = toast.clone();
        let submitting = submitting.clone();
        let reconnecting = reconnecting.clone();

        Callback::from(move |_| {
            let conn_type = *connection_type;
            let ip_m = *ip_mode;
            let ssid_val = (*ssid).clone();
            let password_val = (*password).clone();
            let ip_addr_val = (*ip_address).clone();
            let gateway_val = (*gateway).clone();
            let net_mask_val = (*net_mask).clone();
            let toast = toast.clone();
            let submitting = submitting.clone();
            let reconnecting = reconnecting.clone();
            let navigator = navigator.clone();

            submitting.set(true);

            spawn_local(async move {
                let ip_config = match ip_m {
                    IpMode::Dhcp => IpConfig::Dhcp,
                    IpMode::Static => {
                        let ip = match Ipv4Addr::from_str(&ip_addr_val) {
                            Ok(v) => v,
                            Err(_) => {
                                toast.set(Some("Invalid IP address".to_string()));
                                submitting.set(false);
                                return;
                            }
                        };
                        let gw = match Ipv4Addr::from_str(&gateway_val) {
                            Ok(v) => v,
                            Err(_) => {
                                toast.set(Some("Invalid gateway address".to_string()));
                                submitting.set(false);
                                return;
                            }
                        };
                        let nm = match Ipv4Addr::from_str(&net_mask_val) {
                            Ok(v) => v,
                            Err(_) => {
                                toast.set(Some("Invalid network mask".to_string()));
                                submitting.set(false);
                                return;
                            }
                        };
                        IpConfig::Static(StaticIpConfig {
                            ip_address: ip,
                            net_mask: nm,
                            gateway: gw,
                        })
                    }
                };

                let config = match conn_type {
                    ConnectionType::Wired => NetworkConfig::Wired(WiredConfig { ip_config }),
                    ConnectionType::Wireless => NetworkConfig::Wireless(WirelessConfig {
                        ssid: ssid_val,
                        password: SecretString::from(password_val),
                        ip_config,
                    }),
                };

                let request = match Request::post("/provision/network").json(&config) {
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
                        // Connection dropped — the device is likely switching networks.
                        // Poll /provision/status until we can confirm the result.
                        reconnecting.set(true);

                        // 45 polls × 2 s = 90 s, enough for the 30 s network test + reconnect.
                        let mut result_msg: Option<String> = None;
                        let mut navigated = false;
                        for _ in 0..45u32 {
                            sleep(Duration::from_secs(2)).await;
                            let Ok(resp) = Request::get("/provision/status").send().await else {
                                continue;
                            };
                            let Ok(status) = resp.json::<ProvisionStatus>().await else {
                                continue;
                            };
                            match status {
                                ProvisionStatus::NetworkConfig => {
                                    result_msg = Some(
                                        "Network configuration could not be applied. \
                                         Please check your settings and try again."
                                            .to_string(),
                                    );
                                }
                                _ => {
                                    navigator.replace(&Route::from(status));
                                    navigated = true;
                                }
                            }
                            break;
                        }

                        if !navigated {
                            toast.set(Some(result_msg.unwrap_or_else(|| {
                                "The device did not reconnect within the expected time. \
                                 Please verify the network settings and try again."
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
                        format!("Network configuration failed ({})", response.status())
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
        <div class="page network-config-page">
            <h1>{ "Network Configuration" }</h1>
            if let Some(msg) = (*toast).clone() {
                <div class="toast toast-error">
                    <span>{ msg }</span>
                    <button onclick={on_dismiss_toast}>{ "✕" }</button>
                </div>
            }
            <div class="form">
                <div class="form-field">
                    <label for="connection-type">{ "Connection type" }</label>
                    <select id="connection-type" onchange={on_connection_change}>
                        <option value="wired" selected=true>{ "Wired" }</option>
                        <option value="wireless">{ "Wireless" }</option>
                    </select>
                </div>

                if *connection_type == ConnectionType::Wireless {
                    <div class="form-field">
                        <label for="ssid">{ "SSID" }</label>
                        <input id="ssid" type="text" placeholder="Network name"
                            value={(*ssid).clone()} oninput={oninput!(ssid)} />
                    </div>
                    <div class="form-field">
                        <label for="password">{ "Password" }</label>
                        <input id="password" type="password" placeholder="Network password"
                            value={(*password).clone()} oninput={oninput!(password)} />
                    </div>
                }

                <div class="form-field">
                    <label>{ "IP configuration" }</label>
                    <div class="radio-group">
                        <label class="radio-label">
                            <input type="radio" name="ip_mode" value="dhcp"
                                checked={*ip_mode == IpMode::Dhcp}
                                onchange={on_ip_mode_change.clone()} />
                            { "DHCP" }
                        </label>
                        <label class="radio-label">
                            <input type="radio" name="ip_mode" value="static"
                                checked={*ip_mode == IpMode::Static}
                                onchange={on_ip_mode_change} />
                            { "Manual" }
                        </label>
                    </div>
                </div>

                if *ip_mode == IpMode::Static {
                    <div class="form-field">
                        <label for="ip-address">{ "IPv4 address" }</label>
                        <input id="ip-address" type="text" placeholder="192.168.1.100"
                            value={(*ip_address).clone()} oninput={oninput!(ip_address)} />
                    </div>
                    <div class="form-field">
                        <label for="gateway">{ "Gateway" }</label>
                        <input id="gateway" type="text" placeholder="192.168.1.1"
                            value={(*gateway).clone()} oninput={oninput!(gateway)} />
                    </div>
                    <div class="form-field">
                        <label for="net-mask">{ "Network mask" }</label>
                        <input id="net-mask" type="text" placeholder="255.255.255.0"
                            value={(*net_mask).clone()} oninput={oninput!(net_mask)} />
                    </div>
                }

                <div class="form-actions">
                    <button class="btn-primary" onclick={on_configure}
                        disabled={*submitting}>
                        if *submitting { { "Please wait…" } } else { { "Configure" } }
                    </button>
                </div>
                if *reconnecting {
                    <p class="reconnect-notice">
                        { "Waiting for the device to reconnect to the provisioning \
                           network. This may take up to a minute…" }
                    </p>
                }
            </div>
        </div>
    }
}
