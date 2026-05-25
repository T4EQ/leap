//! Main application entry point and router configuration.
//!
//! This module defines the application's routing structure using [`Route`]
//! and sets up the top-level component tree, including the [`crate::context::ContentProvider`]
//! for application-wide state management.

use yew::prelude::*;
use yew_router::prelude::*;

use crate::context::ContentProvider;
use crate::pages::dashboard::Dashboard;
use crate::pages::player::VideoPlayer;
use crate::pages::status::StatusDashboard;

/// Represents the possible routes in the application.
#[derive(Debug, Clone, PartialEq, Routable)]
pub enum Route {
    /// The home page, displaying the [`crate::pages::dashboard::Dashboard`].
    #[at("/")]
    Home,

    /// A playlist view, identified by its unique ID.
    #[at("/playlists/:playlist_id")]
    Playlist { playlist_id: usize },

    /// A video playback view within a specific playlist.
    #[at("/playlists/:playlist_id/videos/:video_id")]
    Video {
        playlist_id: usize,
        video_id: String,
    },

    /// The application status dashboard.
    #[at("/status")]
    Status,
}

fn switch(route: Route) -> Html {
    match route {
        Route::Home => {
            html! {
                <Dashboard>
                </Dashboard>
            }
        }
        Route::Playlist { playlist_id } => {
            html! {
                <VideoPlayer playlist_id={playlist_id} video_id={None as Option<String>}>
                </VideoPlayer>
            }
        }
        Route::Video {
            playlist_id,
            video_id,
        } => {
            html! {
                <VideoPlayer playlist_id={playlist_id} video_id={Some(video_id)}>
                </VideoPlayer>
            }
        }
        Route::Status => {
            html! {
                <StatusDashboard>
                </StatusDashboard>
            }
        }
    }
}

/// The main application component.
///
/// This component initializes the router and wraps the application in a [`crate::context::ContentProvider`].
#[function_component(App)]
pub fn app() -> Html {
    html! {
        <ContentProvider>
            <BrowserRouter>
                <Switch<Route> render={switch} />
            </BrowserRouter>
        </ContentProvider>
    }
}
