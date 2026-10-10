//! Decorative setup icons, shared by QR loading and the configuration pages.
use yew::prelude::*;

/// An icon of a picture in SVG.
#[function_component(ImageIcon)]
pub fn image_icon() -> Html {
    html! {
        <svg class="choice-icon" viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" aria-hidden="true">
            <rect x="5" y="6" width="38" height="36" rx="3" />
            <circle cx="16" cy="17" r="4" />
            <path d="M6 36l11-12 8 8 7-9 11 13" />
        </svg>
    }
}

/// An icon of a camera in SVG.
#[function_component(CameraIcon)]
pub fn camera_icon() -> Html {
    html! {
        <svg class="choice-icon" viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" aria-hidden="true">
            <path d="M5 14h9l4-6h12l4 6h9v27H5z" />
            <circle cx="24" cy="27" r="9" />
        </svg>
    }
}

/// An icon of a lock in SVG.
#[function_component(LockIcon)]
pub fn lock_icon() -> Html {
    html! {
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <rect x="5" y="10" width="14" height="11" rx="2" />
            <path d="M8 10V6a4 4 0 0 1 8 0v4" />
        </svg>
    }
}
