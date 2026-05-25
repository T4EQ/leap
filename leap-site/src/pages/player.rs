//! This module contains the video player page component.
//!
//! It allows users to view a specific video within a playlist. It also provides
//! a list of other videos in the same playlist for easy navigation.

use crate::context::ContentContextHandle;
use gloo_net::http::Request;
use leap_api::api::content::meta::get::VideoStatus::{Downloaded, Downloading, Failed, Pending};
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

#[derive(yew::Properties, PartialEq, Eq)]
pub struct VideoPlayerProps {
    pub playlist_id: usize,
    pub video_id: Option<String>,
}

#[function_component(VideoPlayer)]
pub fn video_player(
    VideoPlayerProps {
        playlist_id,
        video_id,
    }: &VideoPlayerProps,
) -> Html {
    let context = use_context::<ContentContextHandle>().expect("ContentContext not found");
    let navigator = use_navigator().expect("Navigator not found");

    {
        let context = context.clone();
        let sections_loaded = context.sections.is_some();
        use_effect_with(
            (*playlist_id, video_id.clone(), sections_loaded),
            move |(playlist_id, video_id, _)| {
                if let Some(sections) = &context.sections
                    && let Some(video_id) = video_id.as_ref()
                    && sections.get(*playlist_id).is_some_and(|s| {
                        s.content
                            .iter()
                            .any(|v| v.id == *video_id && v.status == Downloaded)
                    })
                {
                    let playlist_id = *playlist_id;
                    let video_id = video_id.clone();
                    let context = context.clone();

                    spawn_local(async move {
                        let Ok(resp) = Request::post(&format!("/api/content/{video_id}/view"))
                            .send()
                            .await
                        else {
                            return;
                        };

                        if !resp.ok() {
                            return;
                        }

                        let Some(sections) = context.sections.as_ref() else {
                            return;
                        };

                        let mut new_sections = (**sections).clone();
                        if let Some(video) = new_sections
                            .get_mut(playlist_id)
                            .and_then(|s| s.content.iter_mut().find(|v| v.id == video_id))
                        {
                            video.view_count += 1;
                            context.dispatch(new_sections);
                        }
                    });
                }
                || ()
            },
        );
    }

    let Some(sections) = &context.sections else {
        return html! {
            <div class={"page"}>
                <p>{"Loading..."}</p>
            </div>
        };
    };

    let Some(section) = sections.get(*playlist_id) else {
        return html! {
            <div class={"page"}>
                <p>{"Invalid playlist."}</p>
            </div>
        };
    };

    let active_video = video_id
        .as_ref()
        .and_then(|video_id| section.content.iter().find(|v| v.id == *video_id));

    if video_id.is_some() && active_video.is_none() {
        return html! {
            <div class={"page"}>
                <p>{"Video not found in this playlist."}</p>
            </div>
        };
    }

    let on_back_click = {
        let navigator = navigator.clone();
        Callback::from(move |_| {
            navigator.back();
        })
    };

    let active_icon = html! {
        <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg">
            <rect x="5" y="10" width="2" height="4" rx="1">
                <animate attributeName="height" values="4;16;4" begin="0s" dur="1.2s" repeatCount="indefinite" />
                <animate attributeName="y" values="10;4;10" begin="0s" dur="1.2s" repeatCount="indefinite" />
            </rect>
            <rect x="11" y="10" width="2" height="4" rx="1">
                <animate attributeName="height" values="4;16;4" begin="0.2s" dur="1.2s" repeatCount="indefinite" />
                <animate attributeName="y" values="10;4;10" begin="0.2s" dur="1.2s" repeatCount="indefinite" />
            </rect>
            <rect x="17" y="10" width="2" height="4" rx="1">
                <animate attributeName="height" values="4;16;4" begin="0.4s" dur="1.2s" repeatCount="indefinite" />
                <animate attributeName="y" values="10;4;10" begin="0.4s" dur="1.2s" repeatCount="indefinite" />
            </rect>
        </svg>
    };

    html! {
        <div class="page player-page">
            <div class="player-main">
                <header>
                    <button class="back-button" onclick={on_back_click}>
                        <svg xmlns="http://www.w3.org/2000/svg" height="30px" viewBox="0 0 24 24" width="24px" fill="#FFFFFF">
                            <path d="M0 0h24v24H0z" fill="none"/>
                            <path d="M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z"/>
                        </svg>
                    </button>
                    <h1>{ &section.name }</h1>
                </header>

                {
                    if let Some(active_video) = active_video && active_video.status == Downloaded {
                        let video_path = format!("/api/content/{}", active_video.id);
                        html!{
                            <div>
                                <video key={active_video.id.clone()} controls=true autoplay=true class="video-player">
                                    <source src={video_path} type="video/mp4" />
                                </video>

                                <h2>{ &active_video.name }</h2>

                                <div class={"details"}>
                                    <span>{ format!("{} views", active_video.view_count) }</span>
                                </div>
                            </div>
                        }
                    } else {
                        html!{}
                    }
                }
            </div>

            <div class={"video-list list"}>
            {
                if section.content.is_empty() {
                    html! {
                        <p>{"No videos in this playlist."}</p>
                    }
                } else {
                    section.content.iter().enumerate().map(|(i, video)| {
                        let (is_downloaded, status_text) = match &video.status {
                            Downloaded => (true, format!("{} views", video.view_count)),
                            Downloading(progress) => (false, format!("Downloading ({:.0}%)", progress.0 * 100.0)),
                            Pending => (false, "Pending".to_string()),
                            Failed(_) => (false, "Download failed".to_string()),
                        };

                        let is_active = active_video.is_some_and(|active| active.id == video.id) && is_downloaded;
                        let icon = if is_active {
                            active_icon.clone()
                        } else {
                            html! { <span>{ format!("{:02}", i + 1) }</span> }
                        };


                        let onclick = if is_downloaded {
                            let navigator = navigator.clone();
                            let playlist_id = *playlist_id;
                            let video_id = video.id.clone();
                            Callback::from(move |_| {
                                navigator.replace(&crate::app::Route::Video { playlist_id , video_id: video_id.clone() });
                            })
                        } else {
                            Callback::noop()
                        };

                        html! {
                            <div {onclick} class={classes!("card", is_active.then_some("active"), (!is_downloaded).then_some("unavailable"))}>
                                <div class="icon">{ icon }</div>
                                <div class="details">
                                    <h3>{ &video.name }</h3>
                                    <span>{ status_text }</span>
                                </div>
                            </div>
                        }
                    }).collect::<Html>()
                }
            }
            </div>
        </div>
    }
}
