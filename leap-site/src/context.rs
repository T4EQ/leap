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

use std::rc::Rc;
use wasm_bindgen::{closure::Closure, JsCast};
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
            let subscription = match web_sys::EventSource::new("/api/content/events") {
                Ok(event_source) => {
                    let on_message: Closure<dyn FnMut(web_sys::MessageEvent)> =
                        Closure::new(move |event: web_sys::MessageEvent| {
                            let Some(data) = event.data().as_string() else {
                                log::error!("Content event did not contain text data");
                                return;
                            };
                            match serde_json::from_str::<Response>(&data) {
                                Ok(response) => context.dispatch(response.videos),
                                Err(e) => log::error!("Failed to decode content event: {e:?}"),
                            }
                        });
                    event_source.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
                    Some((event_source, on_message))
                }
                Err(e) => {
                    log::error!("Failed to subscribe to content events: {e:?}");
                    None
                }
            };

            move || {
                if let Some((event_source, on_message)) = subscription {
                    event_source.set_onmessage(None);
                    event_source.close();
                    drop(on_message);
                }
            }
        });
    }

    html! {
        <ContextProvider<ContentContextHandle> context={context}>
            { props.children.clone() }
        </ContextProvider<ContentContextHandle>>
    }
}
