use crate::config_import::{MAX_CONFIG_BYTES, decode_qr, parse_config};
use gloo_timers::future::sleep;
use leap_api::types::LeapConfig;
use std::{cell::Cell, rc::Rc, time::Duration};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, HtmlInputElement, HtmlVideoElement, MediaStream,
};
use yew::prelude::*;

fn canvas(
    width: u32,
    height: u32,
) -> Result<(HtmlCanvasElement, CanvasRenderingContext2d), JsValue> {
    let canvas: HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")?
        .dyn_into()?;
    // Bound QR decoding work while preserving enough detail for dense codes.
    let scale = (1600.0 / width.max(height).max(1) as f64).min(1.0);
    canvas.set_width((width as f64 * scale).max(1.0) as u32);
    canvas.set_height((height as f64 * scale).max(1.0) as u32);
    let context = canvas.get_context("2d")?.unwrap().dyn_into()?;
    Ok((canvas, context))
}

fn read_canvas(
    canvas: &HtmlCanvasElement,
    context: &CanvasRenderingContext2d,
) -> Result<LeapConfig, &'static str> {
    let rgba = context
        .get_image_data(0.0, 0.0, canvas.width() as f64, canvas.height() as f64)
        .map_err(|_| "Could not read image pixels.")?
        .data();
    let gray: Vec<u8> = rgba
        .chunks_exact(4)
        .map(|p| {
            let luminance = (299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32) / 1000;
            ((luminance * p[3] as u32 + 255 * (255 - p[3] as u32)) / 255) as u8
        })
        .collect();
    decode_qr(canvas.width() as usize, canvas.height() as usize, &gray)
}

async fn read_file(file: web_sys::File, image: bool) -> Result<LeapConfig, &'static str> {
    let limit = if image {
        10 * 1024 * 1024
    } else {
        MAX_CONFIG_BYTES
    };
    if file.size() > limit as f64 {
        return Err("File is too large. Maximum: 10 MiB for images, 64 KiB for configuration.");
    }
    if !image {
        let text = JsFuture::from(file.text())
            .await
            .map_err(|_| "Could not read configuration file.")?
            .as_string()
            .ok_or("Could not read configuration text.")?;
        return parse_config(&text, file.name().to_ascii_lowercase().ends_with(".toml"));
    }
    let window = web_sys::window().unwrap();
    let promise = window
        .create_image_bitmap_with_blob(&file)
        .map_err(|_| "Could not open image.")?;
    let bitmap: web_sys::ImageBitmap = JsFuture::from(promise)
        .await
        .map_err(|_| "Unsupported or invalid image. Try PNG or JPEG.")?
        .dyn_into()
        .map_err(|_| "Could not decode image.")?;
    let result = (|| {
        if bitmap.width() as u64 * bitmap.height() as u64 > 40_000_000 {
            return Err("Image exceeds the 40 megapixel limit.");
        }
        let (canvas, context) = canvas(bitmap.width(), bitmap.height())
            .map_err(|_| "Could not create image canvas.")?;
        context
            .draw_image_with_image_bitmap_and_dw_and_dh(
                &bitmap,
                0.0,
                0.0,
                canvas.width() as f64,
                canvas.height() as f64,
            )
            .map_err(|_| "Could not draw image.")?;
        read_canvas(&canvas, &context)
    })();
    bitmap.close();
    result
}

fn stop_stream(stream: &MediaStream) {
    for track in stream.get_tracks().iter() {
        if let Ok(track) = track.dyn_into::<web_sys::MediaStreamTrack>() {
            track.stop();
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct Props {
    pub onimport: Callback<LeapConfig>,
    pub disabled: bool,
}

#[function_component(ImportControls)]
pub fn import_controls(props: &Props) -> Html {
    let message = use_state(|| None::<String>);
    let busy = use_state(|| false);
    let scanning = use_state(|| false);
    let video_ref = use_node_ref();
    let generation = use_mut_ref(|| Rc::new(Cell::new(0u64)));
    let stream = use_mut_ref(|| None::<MediaStream>);
    {
        let generation = generation.clone();
        let stream = stream.clone();
        use_effect_with((), move |_| {
            move || {
                generation.borrow().set(generation.borrow().get() + 1);
                if let Some(stream) = stream.borrow_mut().take() {
                    stop_stream(&stream);
                }
            }
        });
    }
    {
        let generation = generation.clone();
        let stream = stream.clone();
        let scanning = scanning.clone();
        let busy = busy.clone();
        use_effect_with(props.disabled, move |disabled| {
            if *disabled {
                generation.borrow().set(generation.borrow().get() + 1);
                if let Some(stream) = stream.borrow_mut().take() {
                    stop_stream(&stream);
                }
                scanning.set(false);
                busy.set(false);
            }
        });
    }
    let onfile = |image: bool| {
        let message = message.clone();
        let busy = busy.clone();
        let generation = generation.clone();
        let onimport = props.onimport.clone();
        Callback::from(move |event: Event| {
            let input = event.target_unchecked_into::<HtmlInputElement>();
            let Some(file) = input.files().and_then(|files| files.get(0)) else {
                return;
            };
            input.set_value("");
            busy.set(true);
            message.set(None);
            let message = message.clone();
            let busy = busy.clone();
            let onimport = onimport.clone();
            let generation = generation.borrow().clone();
            let current = generation.get();
            spawn_local(async move {
                let result = read_file(file, image).await;
                if generation.get() != current {
                    return;
                }
                match result {
                    Ok(config) => {
                        onimport.emit(config);
                        message.set(Some("Configuration imported. Review the fields below, then choose Configure to apply.".into()));
                    }
                    Err(error) => message.set(Some(error.into())),
                }
                busy.set(false);
            });
        })
    };
    let onstop = {
        let generation = generation.clone();
        let stream = stream.clone();
        let scanning = scanning.clone();
        Callback::from(move |_| {
            generation.borrow().set(generation.borrow().get() + 1);
            if let Some(stream) = stream.borrow_mut().take() {
                stop_stream(&stream);
            }
            scanning.set(false);
        })
    };
    let onscan = {
        let generation = generation.clone();
        let stream = stream.clone();
        let scanning = scanning.clone();
        let message = message.clone();
        let video_ref = video_ref.clone();
        let onimport = props.onimport.clone();
        Callback::from(move |_| {
            let window = web_sys::window().unwrap();
            if !window.is_secure_context() {
                message.set(Some("Live camera scanning requires trusted HTTPS. Import a QR image or configuration file instead.".into()));
                return;
            }
            scanning.set(true);
            message.set(Some(
                "Allow camera access and point the camera at a configuration QR code.".into(),
            ));
            let generation = generation.borrow().clone();
            let current = generation.get();
            let stream = stream.clone();
            let scanning = scanning.clone();
            let message = message.clone();
            let video_ref = video_ref.clone();
            let onimport = onimport.clone();
            spawn_local(async move {
                let result = async {
                    let constraints = web_sys::MediaStreamConstraints::new();
                    constraints.set_audio(&JsValue::FALSE);
                    // A preference, not a requirement: desktop webcams remain usable.
                    let video = serde_json::json!({"facingMode": "environment"});
                    let video = web_sys::js_sys::JSON::parse(&video.to_string()).unwrap();
                    constraints.set_video(&video);
                    let devices = window.navigator().media_devices().map_err(|_| "Camera unavailable. Import a file instead.")?;
                    let promise = devices.get_user_media_with_constraints(&constraints).map_err(|_| "Could not request camera access.")?;
                    let media: MediaStream = JsFuture::from(promise).await.map_err(|_| "Camera access denied or unavailable. Import a file or try again.")?
                        .dyn_into().map_err(|_| "Could not open camera.")?;
                    if generation.get() != current { stop_stream(&media); return Ok(()); }
                    // MediaStream::clone() clones the tracks; retain the same JS stream instead.
                    *stream.borrow_mut() = Some(Clone::clone(&media));
                    let video = video_ref.cast::<HtmlVideoElement>().ok_or("Could not display camera preview.")?;
                    video.set_muted(true);
                    video.set_src_object(Some(&media));
                    JsFuture::from(video.play().map_err(|_| "Could not start camera preview.")?).await.map_err(|_| "Could not play camera preview.")?;
                    while generation.get() == current {
                        if video.video_width() > 0 {
                            let (canvas, context) = canvas(video.video_width(), video.video_height()).map_err(|_| "Could not create camera canvas.")?;
                            context.draw_image_with_html_video_element_and_dw_and_dh(&video, 0.0, 0.0, canvas.width() as f64, canvas.height() as f64)
                                .map_err(|_| "Could not read camera frame.")?;
                            match read_canvas(&canvas, &context) {
                                Ok(config) => {
                                    onimport.emit(config);
                                    message.set(Some("Configuration imported. Review the fields below, then choose Configure to apply.".into()));
                                    break;
                                }
                                Err(error) => message.set(Some(error.into())),
                            }
                        }
                        sleep(Duration::from_millis(300)).await;
                    }
                    Ok::<(), &'static str>(())
                }.await;
                if generation.get() == current {
                    if let Some(media) = stream.borrow_mut().take() {
                        stop_stream(&media);
                    }
                    scanning.set(false);
                    if let Err(error) = result {
                        message.set(Some(error.into()));
                    }
                }
            });
        })
    };
    let disabled = props.disabled || *busy || *scanning;
    html! {
        <section class="config-import" aria-label="Import configuration">
            <h2>{"Import configuration"}</h2>
            <p>{"Use settings supplied by your administrator, or fill in the form below. Files and QR codes contain credentials; keep them private."}</p>
            <div class="form-field">
                <label for="config-file">{"Configuration file (JSON or TOML)"}</label>
                <input id="config-file" type="file" accept=".json,.toml,application/json" onchange={onfile(false)} disabled={disabled} />
            </div>
            <div class="form-field">
                <label for="qr-file">{"QR image (JSON payload)"}</label>
                <input id="qr-file" type="file" accept="image/*" onchange={onfile(true)} disabled={disabled} />
            </div>
            if *scanning {
                <button class="btn-primary" onclick={onstop}>{"Stop camera"}</button>
            } else {
                <button class="btn-primary" onclick={onscan} disabled={disabled}>{"Scan with camera"}</button>
            }
            <video ref={video_ref} hidden={!*scanning} autoplay=true muted=true playsinline=true aria-label="QR camera preview" />
            if *busy { <p role="status">{"Reading file…"}</p> }
            if let Some(message) = &*message { <p role="status">{message}</p> }
        </section>
    }
}
