//! Utility macros and helper functions.
//!
//! This module provides common utilities used throughout the application.

/// A macro to create a Yew `Callback` for handling `input` events on `HtmlInputElement`s.
///
/// This macro simplifies updating a `UseStateHandle<String>` when the input value changes.
///
/// # Example
///
/// ```rust
/// use yew::prelude::*;
/// use leap_provision_site::oninput;
///
/// #[function_component(MyInputElem)]
/// pub fn my_input_elem() -> Html {
///     let my_state_handle = use_state(String::new);
///     html! {
///         <input type="text" oninput={oninput!(my_state_handle)} />
///     }
/// }
/// ```
#[macro_export]
macro_rules! oninput {
    ($state:expr) => {{
        let state = $state.clone();
        Callback::from(move |e: InputEvent| {
            state.set(
                e.target_unchecked_into::<::web_sys::HtmlInputElement>()
                    .value(),
            );
        })
    }};
}
