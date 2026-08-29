//! `/search/relationships/:code` (KAIROS-T-0089): the route shell for the
//! focal flight-level graph — the old five-panel explorer is gone; the
//! canvas and its side/manage panels live in [`super::graph`]. Re-mounts
//! when `:code` changes (refocus navigates here with a `?trail=`).

use aurora_dark::components::PageHeader;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use super::graph::GraphView;

/// The route wrapper; the view's question is the page subtitle.
#[component]
pub fn RelationshipsPage() -> impl IntoView {
    let params = use_params_map();
    view! {
        {move || {
            let code = params
                .read()
                .get("code")
                .unwrap_or_default()
                .to_uppercase();
            view! {
                <PageHeader
                    title=code.clone()
                    sub="What does this depend on, what does it feed into, where does it sit?"
                />
                <GraphView short_code=code/>
            }
        }}
    }
}
