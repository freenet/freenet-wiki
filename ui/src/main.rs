//! Freenet Wiki UI
//!
//! A Dioxus-based web interface for the decentralized wiki.

use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        div {
            class: "wiki-app",
            h1 { "Freenet Wiki" }
            p { "Loading..." }
        }
    }
}
