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

#[component(WifiIcon)]
pub fn wifi_icon() -> Html {
    html! {
        <svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" role="img" aria-hidden="true">
            <path d="M6 18a25.46 25.46 0 0 1 36 0M12 24a16.97 16.97 0 0 1 24 0M18 30a8.49 8.49 0 0 1 12 0" />
            <circle cx="24" cy="36" r="2" fill="currentColor" stroke="none" />
        </svg>
    }
}

#[component(BookPlayIcon)]
pub fn book_play_icon() -> Html {
    html! {
        <svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" role="img" aria-hidden="true">
          <path d="M24 12C18 7 11 7 5 10v27c6-3 13-3 19 2 6-5 13-5 19-2V10c-6-3-13-3-19 2Z M24 12v27" />
          <path d="m31 19 7 4.5-7 4.5Z" fill="currentColor" stroke="none" />
        </svg>
    }
}
