use dioxus::prelude::*;

#[component]
pub fn Navbar() -> Element {
    rsx! {
        div { class: "navbar",
            span { class: "navbar-title", "Chlorophyll" }
        }
        Outlet::<crate::Route> {}
    }
}
