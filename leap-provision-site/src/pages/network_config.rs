//! Component for configuring network settings.
//!
//! This component provides a form to configure:
//! - **Connection type**: Wired or Wireless.
//! - **Wireless settings**: SSID and password (if wireless is selected).
//! - **IP configuration**: DHCP or Static IP (with IP, gateway, and netmask).
//!
//! When configured, the settings are sent to the server at `/provision/network`.
//! The component handles the asynchronous submission and manages the connection
//! state during the device's reconfiguration and potential network switch.

use crate::app::{Route, use_provision_redirect};
use crate::components::icons::{LockIcon, WifiIcon};
use crate::components::qr_form::{QrData, QrForm};
use crate::network::parse_wifi_qr_code_settings;

use gloo_net::http::Request;
use gloo_timers::future::sleep;
use leap_api::types::{
    IpConfig, NetworkConfig, ProvisionStatus, StaticIpConfig, WiredConfig, WirelessConfig,
};
use secrecy::{ExposeSecret, SecretString};
use std::net::{AddrParseError, Ipv4Addr};
use std::str::FromStr;
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;
use web_sys::{FormData, HtmlFormElement, HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;
use yew_router::prelude::*;

#[derive(Copy, Clone, PartialEq, Eq, Default)]
enum ConnectionType {
    #[default]
    Wired,
    Wireless,
}

impl ConnectionType {
    /// Maps the value of the connection type form control to a [`ConnectionType`].
    fn from_form_value(value: &str) -> Self {
        match value {
            "wireless" => ConnectionType::Wireless,
            _ => ConnectionType::Wired,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Default)]
enum IpMode {
    #[default]
    Dhcp,
    Static,
}

impl IpMode {
    /// Maps the value of the IP mode form control to an [`IpMode`].
    fn from_form_value(value: &str) -> Self {
        match value {
            "static" => IpMode::Static,
            _ => IpMode::Dhcp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
enum NetworkConfigError {
    #[error("Invalid IP address: {0}")]
    IpAddr(AddrParseError),
    #[error("Invalid network gateway: {0}")]
    Gateway(AddrParseError),
    #[error("Invalid network mask: {0}")]
    NetMask(AddrParseError),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum NetworkState {
    Idle,
    Submitting,
    Reconnecting,
}

const FORM_ITEM_CONNECTION_TYPE: &str = "connection-type";
const FORM_ITEM_IP_MODE: &str = "ip-mode";
const FORM_ITEM_SSID: &str = "ssid";
const FORM_ITEM_PASSWD: &str = "password";
const FORM_ITEM_IP_ADDR: &str = "ip-address";
const FORM_ITEM_GATEWAY: &str = "gateway";
const FORM_ITEM_NET_MASK: &str = "net-mask";

const IPADDR_PATTERN: &str = "([0-9]{1,3}[.]){3}[0-9]{1,3}";

/// Builds the [`NetworkConfig`] to submit from the values currently held by the form.
fn network_config_from_form(form: HtmlFormElement) -> Result<NetworkConfig, NetworkConfigError> {
    let form = FormData::new_with_form(&form).unwrap();
    let extract_form_value = |name| form.get(name).as_string().unwrap_or_default();
    let connection_type =
        ConnectionType::from_form_value(&extract_form_value(FORM_ITEM_CONNECTION_TYPE));
    let ip_mode = IpMode::from_form_value(&extract_form_value(FORM_ITEM_IP_MODE));
    let ip_config = match ip_mode {
        IpMode::Dhcp => IpConfig::Dhcp,
        IpMode::Static => {
            let extract_ip =
                |name: &'static str, error_ctor: fn(AddrParseError) -> NetworkConfigError| {
                    Ipv4Addr::from_str(&extract_form_value(name)).map_err(error_ctor)
                };
            let ip = extract_ip(FORM_ITEM_IP_ADDR, NetworkConfigError::IpAddr)?;
            let gw = extract_ip(FORM_ITEM_GATEWAY, NetworkConfigError::Gateway)?;
            let nm = extract_ip(FORM_ITEM_NET_MASK, NetworkConfigError::NetMask)?;
            IpConfig::Static(StaticIpConfig {
                ip_address: ip,
                net_mask: nm,
                gateway: gw,
            })
        }
    };

    Ok(match connection_type {
        ConnectionType::Wired => NetworkConfig::Wired(WiredConfig { ip_config }),
        ConnectionType::Wireless => NetworkConfig::Wireless(WirelessConfig {
            ssid: extract_form_value(FORM_ITEM_SSID),
            password: SecretString::from(extract_form_value(FORM_ITEM_PASSWD)),
            ip_config,
        }),
    })
}

/// Sends the network configuration to the server and follows the provisioning
/// status until the device has either applied or rejected it.
fn submit_network_configuration(
    net_config: NetworkConfig,
    state: UseStateHandle<NetworkState>,
    toast: UseStateHandle<Option<String>>,
    navigator: Navigator,
) {
    state.set(NetworkState::Submitting);

    spawn_local(async move {
        // TODO: Update how the request is handled today, which is brittle and attempts to infer
        // that the device probably disconnected, but there's no guarantee that the device even
        // got the request.
        // The API between the frontend and the backend needs to be updated for this.
        let request = match Request::post("/provision/network").json(&net_config) {
            Ok(r) => r,
            Err(e) => {
                toast.set(Some(format!("Failed to serialize request: {e}")));
                state.set(NetworkState::Idle);
                return;
            }
        };

        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => {
                // Connection dropped — the device is likely switching networks.
                // Poll /provision/status until we can confirm the result.
                log::warn!("POST /provision/network failed, waiting for the device: {e}");
                state.set(NetworkState::Reconnecting);

                // 45 polls × 2 s = 90 s, enough for the 30 s network test + reconnect.
                let mut result_msg: Option<String> = None;
                let mut navigated = false;
                for _ in 0..45 {
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

                    if status == ProvisionStatus::NetworkConfig {
                        result_msg = Some(
                            "Network configuration could not be applied. \
                                Please check your settings and try again."
                                .to_string(),
                        );
                    } else {
                        log::info!("Network configuration was successfully applied.");
                        navigator.replace(&Route::from(status));
                        navigated = true;
                    }
                    break;
                }

                if !navigated {
                    let result_msg = result_msg.unwrap_or_else(|| {
                        "The device did not reconnect within the expected time. \
                         Please verify the network settings and try again."
                            .to_string()
                    });
                    log::error!("Failed to apply network config: {result_msg}");
                    toast.set(Some(result_msg));
                }
                state.set(NetworkState::Idle);
                return;
            }
        };

        if !response.ok() {
            let body = response.text().await.unwrap_or_default();
            log::warn!("Network configuration failed: ({response:?}): {body}");
            toast.set(Some(if body.is_empty() {
                format!("Network configuration failed ({})", response.status())
            } else {
                body
            }));
            state.set(NetworkState::Idle);
            return;
        }

        if let Ok(status_resp) = Request::get("/provision/status").send().await
            && let Ok(status) = status_resp.json::<ProvisionStatus>().await
        {
            log::info!("Network configuration was successfully applied.");
            navigator.replace(&Route::from(status));
        }

        state.set(NetworkState::Idle);
    });
}

fn on_connection_change_callback(
    connection_type: UseStateHandle<ConnectionType>,
) -> Callback<Event> {
    Callback::from(move |e: Event| {
        let select = e.target_unchecked_into::<HtmlSelectElement>();
        connection_type.set(ConnectionType::from_form_value(&select.value()));
    })
}

fn on_ip_mode_change_callback(ip_mode: UseStateHandle<IpMode>) -> Callback<Event> {
    Callback::from(move |e: Event| {
        let input = e.target_unchecked_into::<HtmlInputElement>();
        ip_mode.set(IpMode::from_form_value(&input.value()));
    })
}

fn on_dismiss_toast_callback(toast: UseStateHandle<Option<String>>) -> Callback<MouseEvent> {
    Callback::from(move |_| toast.set(None))
}

fn on_configure_callback(
    toast: UseStateHandle<Option<String>>,
    state: UseStateHandle<NetworkState>,
    navigator: Navigator,
) -> Callback<SubmitEvent> {
    Callback::from(move |e: SubmitEvent| {
        e.prevent_default();

        let net_config = match network_config_from_form(e.target_unchecked_into()) {
            Ok(cfg) => cfg,
            Err(err) => {
                toast.set(Some(format!("Invalid configuration: {err}")));
                state.set(NetworkState::Idle);
                return;
            }
        };
        submit_network_configuration(net_config, state.clone(), toast.clone(), navigator.clone());
    })
}

fn on_advanced_toggle_callback(advanced_open: UseStateHandle<bool>) -> Callback<Event> {
    Callback::from(move |event: Event| {
        let element = event.target_unchecked_into::<web_sys::Element>();
        advanced_open.set(element.has_attribute("open"));
    })
}

fn on_qr_loaded_callback(
    imported: UseStateHandle<(usize, Option<WirelessConfig>)>,
    connection_type: UseStateHandle<ConnectionType>,
) -> Callback<QrData, Result<(), Box<dyn std::error::Error>>> {
    Callback::from(move |qr_data: QrData| {
        let payload = qr_data.payload;
        let wifi_str = std::str::from_utf8(&payload)?;
        let settings = parse_wifi_qr_code_settings(wifi_str).inspect_err(|e| {
            log::error!("Failed to parse WiFi QR settings: {e}");
        })?;
        log::info!("Settings: {settings:?}");

        let generation = imported.0;
        imported.set((generation + 1, Some(settings)));
        connection_type.set(ConnectionType::Wireless);

        Ok(())
    })
}

/// The component for configuring network settings.
#[function_component(NetworkConfigPage)]
pub fn network_config_page() -> Html {
    use_provision_redirect(Route::NetworkConfig);

    // State variables
    let connection_type = use_state(ConnectionType::default);
    let ip_mode = use_state(IpMode::default);
    let toast: UseStateHandle<Option<String>> = use_state(|| None);
    let state = use_state(|| NetworkState::Idle);
    let imported = use_state(|| (0usize, None::<WirelessConfig>));
    let advanced_open = use_state(|| false);
    let navigator = use_navigator().unwrap();

    // Callbacks
    let on_connection_change = on_connection_change_callback(connection_type.clone());
    let on_ip_mode_change = on_ip_mode_change_callback(ip_mode.clone());
    let on_dismiss_toast = on_dismiss_toast_callback(toast.clone());
    let on_configure = on_configure_callback(toast.clone(), state.clone(), navigator);
    let on_advanced_toggle = on_advanced_toggle_callback(advanced_open.clone());
    let on_qr_loaded = on_qr_loaded_callback(imported.clone(), connection_type.clone());

    let wifi_cfg = imported.1.as_ref();
    html! {
        <div class="page network-config-page">
            <h1> <WifiIcon/> { "Connect to the Internet" }</h1>
            if let Some(msg) = (*toast).clone() {
                <div class="toast toast-error">
                    <span>{ msg }</span>
                    <button onclick={on_dismiss_toast}>{ "✕" }</button>
                </div>
            }

            <QrForm disabled=false callback={on_qr_loaded}/>

            if wifi_cfg.is_some() {
                <div class="settings-ready" role="status">
                    {"✓ Your Wi-Fi settings are ready."}
                </div>
            }
            if wifi_cfg.is_some() || *advanced_open {
                <button type="submit" form="network-settings" class="btn-primary apply-settings" disabled={*state != NetworkState::Idle}>
                    if *state != NetworkState::Idle { {"Applying settings…"} } else { {"Apply settings"} }
                </button>
                <p class="connection-notice">{"The device will check the connection. You may briefly lose contact."}</p>
            }
            if *state == NetworkState::Reconnecting {
                <p class="reconnect-notice">{"Waiting for LEAP to reconnect. This may take up to a minute…"}</p>
            }

            <p class="qr-privacy"><LockIcon/>{"Do not share your QR code."}</p>

           <details class="advanced-settings" ontoggle={on_advanced_toggle}>
                <summary>{"Advanced: enter settings manually"}</summary>

                // The form key is mutated when a new form is imported so that the form is recreated,
                // reapplying the defaults (which correspond to the imported wifi config)
                <form onsubmit={on_configure} id="network-settings" key={imported.0.to_string()}>
                    <fieldset class="form" disabled={*state != NetworkState::Idle}>
                        <div class="form-field">
                            <label for={FORM_ITEM_CONNECTION_TYPE}>{ "Connection type" }</label>
                            <select id={FORM_ITEM_CONNECTION_TYPE} name={FORM_ITEM_CONNECTION_TYPE} onchange={on_connection_change}>
                                <option value="wired" selected={*connection_type == ConnectionType::Wired}>{ "Wired network" }</option>
                                <option value="wireless" selected={*connection_type == ConnectionType::Wireless}>{ "Wi-Fi" }</option>
                            </select>
                        </div>

                        if *connection_type == ConnectionType::Wireless {
                            <div class="form-field">
                                <label for={FORM_ITEM_SSID}>{ "Network name" }</label>
                                <input id={FORM_ITEM_SSID} name={FORM_ITEM_SSID} type="text" placeholder="Network name" required=true
                                    ~defaultValue={wifi_cfg.map_or(String::new(), |config| config.ssid.clone())} />
                            </div>
                            <div class="form-field">
                                <label for={FORM_ITEM_PASSWD}>{ "Password" }</label>
                                <input id={FORM_ITEM_PASSWD} name={FORM_ITEM_PASSWD} type="password" placeholder="Network password (optional)"
                                     ~defaultValue={wifi_cfg.map_or(String::new(), |config| config.password.expose_secret().to_owned())} />
                            </div>
                        }

                        <div class="form-field">
                            <label>{ "IP settings" }</label>
                            <div class="radio-group">
                                <label class="radio-label">
                                    <input type="radio" name={FORM_ITEM_IP_MODE} value="dhcp"
                                        checked={*ip_mode == IpMode::Dhcp}
                                        onchange={on_ip_mode_change.clone()} />
                                    { "Automatic" }
                                </label>
                                <label class="radio-label">
                                    <input type="radio" name={FORM_ITEM_IP_MODE} value="static"
                                        checked={*ip_mode == IpMode::Static}
                                        onchange={on_ip_mode_change} />
                                    { "Static" }
                                </label>
                            </div>
                        </div>

                        if *ip_mode == IpMode::Static {
                            <div class="form-field">
                                <label for={FORM_ITEM_IP_ADDR}>{ "IPv4 address" }</label>
                                <input id={FORM_ITEM_IP_ADDR} name={FORM_ITEM_IP_ADDR} type="text" placeholder="192.168.1.100"
                                    required=true pattern={IPADDR_PATTERN} />
                            </div>
                            <div class="form-field">
                                <label for={FORM_ITEM_GATEWAY}>{ "Gateway" }</label>
                                <input id={FORM_ITEM_GATEWAY} name={FORM_ITEM_GATEWAY} type="text" placeholder="192.168.1.1"
                                    required=true pattern={IPADDR_PATTERN} />
                            </div>
                            <div class="form-field">
                                <label for={FORM_ITEM_NET_MASK}>{ "Network mask" }</label>
                                <input id={FORM_ITEM_NET_MASK} name={FORM_ITEM_NET_MASK} type="text" placeholder="255.255.255.0"
                                    required=true pattern={IPADDR_PATTERN} />
                            </div>
                        }
                    </fieldset>
                </form>
            </details>
        </div>
    }
}
