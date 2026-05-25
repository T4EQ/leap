//! This module provides the context for managing and providing content metadata throughout the application.
//!
//! It uses [`ContentContext`] to hold the playlists and [`ContentProvider`] as a context provider component
//! that loads and keeps the [`ContentContext`] state.
//!
//! # Usage Guide
//!
//! Wrap your application (or a part of it) with the [`ContentProvider`] component to make the content
//! metadata available via [`use_context`] in child components.
//!
//! ```rust,no_run
//! use yew::prelude::*;
//! use leap_site::context::{ContentProvider};
//!
//! #[function_component(MyComponent)]
//! pub fn my_component() -> Html {
//!     /// ...
//! #   todo!()
//! }
//!
//! fn App() -> Html {
//!     html! {
//!         <ContentProvider>
//!             <MyComponent />
//!         </ContentProvider>
//!     }
//! }
//! ```

use gloo_net::http::Request;
use std::rc::Rc;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use leap_api::api::content::meta::get::{GroupedSection, Response};

#[derive(Clone, Debug, PartialEq)]
pub struct ContentContext {
    pub sections: Option<Rc<Vec<GroupedSection>>>,
}

impl Reducible for ContentContext {
    type Action = Vec<GroupedSection>;

    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        Rc::new(Self {
            sections: Some(Rc::new(action)),
        })
    }
}

pub type ContentContextHandle = UseReducerHandle<ContentContext>;

#[derive(Properties, PartialEq)]
pub struct ContentProviderProps {
    #[prop_or_default]
    pub children: Html,
}

#[function_component(ContentProvider)]
pub fn content_provider(props: &ContentProviderProps) -> Html {
    let context = use_reducer(|| ContentContext { sections: None });

    {
        let context = context.clone();
        use_effect_with((), move |_| {
            if context.sections.is_none() {
                let context = context.clone();
                spawn_local(async move {
                    if let Some(sections) = fetch_sections().await {
                        context.dispatch(sections);
                    }
                });
            }
            || ()
        });
    }

    html! {
        <ContextProvider<ContentContextHandle> context={context}>
            { props.children.clone() }
        </ContextProvider<ContentContextHandle>>
    }
}

async fn fetch_sections() -> Option<Vec<GroupedSection>> {
    let response = match Request::get("/api/content/meta").send().await {
        Ok(v) => v,
        Err(e) => {
            log::error!("Failed to fetch content meta. Error performing HTTP request: {e:?}");
            return None;
        }
    };

    let response = match response.json::<Response>().await {
        Ok(v) => v,
        Err(e) => {
            log::error!("Failed to fetch videos. Error decoding json: {e:?}");
            return None;
        }
    };

    Some(response.videos)
}
