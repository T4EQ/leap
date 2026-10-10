//! Handling of QR codes in the LEAP provisioning application.

use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use web_sys::js_sys::Uint8Array;
use yew::prelude::*;

use crate::components::icons::CameraIcon;
use crate::components::icons::ImageIcon;

#[derive(Debug, thiserror::Error)]
pub enum QrLoadError {
    #[error("Could not read image: {0:?}")]
    OpenImage(JsValue),
    #[error("Unsupported or invalid image: {0}")]
    DecodeImage(image::ImageError),
    #[error("No QR code present in the image")]
    NoQrPresent,
    #[error("QR failed to extract: {0}")]
    ExtractQr(quircs::ExtractError),
    #[error("QR failed to decode: {0}")]
    DecodeQr(quircs::DecodeError),
    #[error("Application-specific error: {0}")]
    AppSpecific(Box<dyn std::error::Error>),
}

pub struct QrData {
    pub payload: Vec<u8>,
}

/// Reads an image file to a canvas, then uses
async fn read_qr_file(file: web_sys::File) -> Result<QrData, QrLoadError> {
    let image_data = wasm_bindgen_futures::JsFuture::from(file.array_buffer())
        .await
        .map_err(QrLoadError::OpenImage)?;
    let image_data = Uint8Array::new(&image_data);

    let v = image_data.to_vec();

    let format = image::guess_format(&v).map_err(QrLoadError::DecodeImage)?;
    let cursor = Cursor::new(v.as_slice());
    let image = image::load(cursor, format).map_err(QrLoadError::DecodeImage)?;

    let luma = image.into_luma8();

    let mut decoder = quircs::Quirc::default();
    let mut codes = decoder.identify(luma.width() as usize, luma.height() as usize, &luma);

    let Some(code) = codes.next() else {
        return Err(QrLoadError::NoQrPresent);
    };

    let code = code.map_err(QrLoadError::ExtractQr)?;
    let data = code.decode().map_err(QrLoadError::DecodeQr)?;
    let payload = data.payload;

    Ok(QrData { payload })
}

#[derive(Default, PartialEq)]
struct LoadState {
    message: Option<String>,
    busy: bool,
}

fn onfile_callback(
    state: UseStateHandle<LoadState>,
    generation: Rc<RefCell<u64>>,
    onimport: Callback<QrData, Result<(), Box<dyn std::error::Error>>>,
    disabled: bool,
) -> Callback<Event> {
    Callback::from(move |event: Event| {
        let (state, generation, onimport) = (state.clone(), generation.clone(), onimport.clone());

        // Also reject a change event already queued when Apply disabled us.
        if disabled {
            return;
        }

        // Get file from the HTML <input> tag
        let input = event.target_unchecked_into::<HtmlInputElement>();
        let Some(file) = input.files().and_then(|files| files.get(0)) else {
            log::warn!("No file to read QR code");
            return;
        };
        input.set_value("");

        state.set(LoadState {
            busy: true,
            message: None,
        });

        *generation.borrow_mut() += 1;
        let current_generation = *generation.borrow();

        // Read file in the background
        spawn_local(async move {
            let result = read_qr_file(file).await;
            // Browser bitmap decoding cannot be cancelled. A newer selection,
            // Apply, or unmount invalidates this ticket. Stale tasks must not
            // mutate the form, clear a newer error, or re-enable its inputs.
            if *generation.borrow() != current_generation {
                return;
            }

            let message = result
                .and_then(|qr_data| onimport.emit(qr_data).map_err(QrLoadError::AppSpecific))
                .err()
                .map(|error| {
                    // Debug contains safe categories/location metadata only;
                    // Display supplies the friendly message for the user.
                    log::debug!("QR import failed: {error:?}");
                    error.to_string()
                });

            state.set(LoadState {
                busy: false,
                message,
            });
        });
    })
}

#[derive(Properties, PartialEq, Debug)]
pub struct QrFormProps {
    /// Disables the loading of QR codes.
    pub disabled: bool,

    /// Called with the resulting QR code.
    pub callback: Callback<QrData, Result<(), Box<dyn std::error::Error>>>,
}

#[component(QrForm)]
pub fn qr_form(props: &QrFormProps) -> Html {
    let state = use_state(LoadState::default);
    let generation = use_mut_ref(|| 0u64);

    {
        let generation = generation.clone();
        let register_cleanup = move |_: &()| {
            let invalidate_pending_decode = move || {
                *generation.borrow_mut() += 1;
            };
            // this function gets registered for cleanup when the dependencies check, which is only
            // when the component is destroyed.
            invalidate_pending_decode
        };
        use_effect_with((), register_cleanup);
    }

    let onfile = onfile_callback(
        state.clone(),
        generation.clone(),
        props.callback.clone(),
        props.disabled,
    );
    let disabled = props.disabled || state.busy;

    html! {
        <section class="qr-options" aria-label="Add settings using a QR code">
            <div class="qr-choice">
                <ImageIcon />
                <div class="choice-content">
                    <h2>{"Open a QR image"}</h2>
                    <p>{"Choose a saved image of your QR code"}</p>
                    <label class={classes!("btn-primary", "image-picker", disabled.then_some("disabled"))}>
                        {"Choose image"}
                        <input id="qr-file" type="file" accept="image/*" aria-label="Choose QR image" onchange={onfile.clone()} disabled={disabled} />
                    </label>
                </div>
            </div>

            <div class="qr-choice">
                <CameraIcon />
                <div class="choice-content">
                    <h2>{"Take a photo of a QR code"}</h2>
                    <p>{"Photograph a printed code or a code on another screen. Keep the whole code in focus."}</p>
                    <p class="capture-note">{"On phones that support it, this opens your camera. Otherwise, choose a photo."}</p>
                    <label class={classes!("btn-primary", "image-picker", disabled.then_some("disabled"))}>
                        {"Take photo"}
                        <input id="qr-photo" type="file" accept="image/*" capture="environment" aria-label="Take QR photo" onchange={onfile} disabled={disabled} />
                    </label>
                </div>
            </div>

            if state.busy { <p role="status">{"Reading your QR image…"}</p> }
            if let Some(message) = &state.message { <p class="import-message" role="status">{message}</p> }
        </section>
    }
}
