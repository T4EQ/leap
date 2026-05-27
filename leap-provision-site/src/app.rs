//! Main application module for the LEAP Provisioning Site.
//!
//! This module defines the routing logic, the main application component,
//! and the primary navigation flow for the provisioning process.
//!
//! It coordinates between various configuration pages (Storage, Network, Leap)
//! and ensures the user is redirected to the correct step based on their
//! current provisioning status.
//!
//! # Routing
//!
//! The application uses [`Route`] to manage navigation between different stages
//! of the provisioning process.
//!
//! # Navigation Flow
//!
//! The application uses the [`use_provision_redirect`] hook to automatically
//! synchronize the client-side route with the actual provisioning status fetched
//! from the server.

use crate::completed::CompletedPage;
use crate::leap_config::LeapConfigPage;
use crate::network_config::NetworkConfigPage;
use crate::storage_config::StorageConfigPage;

use gloo_net::http::Request;
use leap_api::types::ProvisionStatus;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

/// Represents the different routes available in the provisioning application.
#[derive(Debug, Clone, PartialEq, Routable)]
pub enum Route {
    /// The initial landing page.
    #[at("/")]
    Start,
    /// The page for configuring storage settings.
    #[at("/storage")]
    StorageConfig,
    /// The page for configuring network settings.
    #[at("/network")]
    NetworkConfig,
    /// The page for configuring LEAP-specific settings.
    #[at("/leap")]
    LeapConfig,
    /// The page shown when provisioning is complete.
    #[at("/completed")]
    Completed,
}

impl From<ProvisionStatus> for Route {
    fn from(value: ProvisionStatus) -> Self {
        match value {
            ProvisionStatus::StorageConfig => Route::StorageConfig,
            ProvisionStatus::NetworkConfig => Route::NetworkConfig,
            ProvisionStatus::LeapConfig => Route::LeapConfig,
            ProvisionStatus::Completed => Route::Completed,
        }
    }
}

/// A hook that synchronizes the current route with the provisioning status from the server.
///
/// This hook fetches the current provisioning status from `/provision/status`
/// and redirects the user to the appropriate route if it differs from the current one.
#[hook]
pub fn use_provision_redirect(current: Route) {
    let navigator = use_navigator().unwrap();
    use_effect_with((), move |_| {
        spawn_local(async move {
            let response = match Request::get("/provision/status").send().await {
                Ok(r) => r,
                Err(e) => {
                    log::error!("Failed to fetch provision status: {e:?}");
                    return;
                }
            };
            match response.json::<ProvisionStatus>().await {
                Ok(status) => {
                    let target = Route::from(status);
                    if target != current {
                        navigator.replace(&target);
                    }
                }
                Err(e) => {
                    log::error!("Failed to parse provision status: {e:?}");
                }
            }
        });
        || ()
    });
}

/// The initial landing page component.
#[function_component(StartPage)]
pub fn start_page() -> Html {
    use_provision_redirect(Route::StorageConfig);

    let navigator = use_navigator().unwrap();
    let on_start = Callback::from(move |_| navigator.replace(&Route::StorageConfig));

    html! {
        <div class="start-page">
            <h1>{ "Welcome to LEAP" }</h1>
            <p>{ "Low-Bandwidth Educational Access Platform" }</p>
            <button class="btn-primary" onclick={on_start}>{ "Start" }</button>
        </div>
    }
}

fn switch(route: Route) -> Html {
    match route {
        Route::Start => html! { <StartPage /> },
        Route::StorageConfig => html! { <StorageConfigPage /> },
        Route::NetworkConfig => html! { <NetworkConfigPage /> },
        Route::LeapConfig => html! { <LeapConfigPage /> },
        Route::Completed => html! { <CompletedPage /> },
    }
}

/// The main application component.
#[function_component(App)]
pub fn app() -> Html {
    html! {
        <BrowserRouter>
            <Switch<Route> render={switch} />
        </BrowserRouter>
    }
}
