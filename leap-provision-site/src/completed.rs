//! Component for the completion page.
//!
//! This component is displayed when the provisioning process has finished successfully.
//! It provides a way for the user to trigger a reboot of the device.

use crate::app::{Route, use_provision_redirect};
use gloo_net::http::Request;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

/// The component displayed when provisioning is successfully completed.
#[function_component(CompletedPage)]
pub fn completed_page() -> Html {
    use_provision_redirect(Route::Completed);

    let rebooting = use_state(|| false);

    let on_reboot = {
        let rebooting = rebooting.clone();
        Callback::from(move |_| {
            rebooting.set(true);
            spawn_local(async {
                // The server reboots without completing this request — ignore the result.
                let _ = Request::post("/provision/complete").send().await;
            });
        })
    };

    html! {
        <div class="page completed-page">
            <h1>{ "Provisioning Complete" }</h1>
            <p>{ "LEAP has been successfully provisioned and is ready to use." }</p>
            if *rebooting {
                <p class="reboot-notice">
                    { "The device is rebooting. Please wait up to 1 minute, then reload " }
                    <a href="http://leap.local">{ "http://leap.local" }</a>
                    { "." }
                </p>
            } else {
                <button class="btn-primary" onclick={on_reboot}>{ "Reboot" }</button>
            }
        </div>
    }
}
