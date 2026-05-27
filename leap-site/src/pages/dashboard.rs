//! This module contains the dashboard page component.
//!
//! It displays a list of available playlists using [`PlaylistsList`], which in turn
//! uses [`PlaylistCard`] to render each individual playlist.

use std::hash::{DefaultHasher, Hasher};
use yew::prelude::*;
use yew_router::prelude::*;

use crate::context::ContentContextHandle;

#[derive(yew::Properties, PartialEq)]
pub struct PlaylistCardProps {
    pub playlist_id: usize,
    pub playlist_name: String,
    pub num_videos: usize,
}

#[function_component(PlaylistCard)]
pub fn playlist_card(
    PlaylistCardProps {
        playlist_id,
        playlist_name,
        num_videos,
    }: &PlaylistCardProps,
) -> Html {
    let navigator = use_navigator();

    let onclick = if *num_videos > 0 {
        let playlist_id = *playlist_id;
        Callback::from(move |_| {
            if let Some(navigator) = &navigator {
                navigator.push(&crate::app::Route::Playlist { playlist_id });
            }
        })
    } else {
        Callback::noop()
    };

    let first_letter = playlist_name.chars().next().unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    hasher.write(playlist_name.as_bytes());
    let hue = hasher.finish() % 360;
    let icon_style = format!("color: hsl({}, 70%, 60%);", hue);

    html! {
        <div {onclick} class={classes!("card", (*num_videos == 0).then_some("unavailable"))}>
            <div class="icon" style={icon_style}>{ first_letter }</div>
            <div class="details">
                <h3>{ playlist_name }</h3>
                <span>{ format!("{} videos", num_videos) }</span>
            </div>
            <div class="arrow"> { "\u{203A}" }</div>
        </div>
    }
}

#[function_component(PlaylistsList)]
pub fn playlists_list() -> Html {
    let context = use_context::<ContentContextHandle>().expect("ContentContext not found");

    let Some(sections) = &context.sections else {
        return html! {
            <p>{"Loading..."}</p>
        };
    };

    if sections.is_empty() {
        html! {
            <p1>{ "No playlists available yet." }</p1>
        }
    } else {
        html! {
                <div class="playlist-list list">
                {
                    sections.iter().enumerate().map(|(index, section)| {
                        let num_videos = section.content.len();
                        html! { <PlaylistCard playlist_id={index} playlist_name={section.name.clone()} num_videos={num_videos} /> }
                    }).collect::<Html>()
                }
                </div>
        }
    }
}

#[function_component(Dashboard)]
pub fn dashboard() -> Html {
    html! {
        <div class="page dashboard-page">
            <header class="header">
                <h1>{ "Playlists" }</h1>
            </header>
            <PlaylistsList/>
        </div>
    }
}
