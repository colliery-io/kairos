//! The preview of a short code: a panel that shows the item when the
//! pointer is on a Kairos code link, a short code that the markdown of a
//! page turned into a link to its item. Other links never open a panel.
//! The setting ([`PeekMode`], in this browser) is one of: hold Shift or
//! Ctrl while the pointer is on the link (the default), plain hover, or no
//! panel. The panel scrolls with the wheel; it stays
//! while the pointer is on it, and it closes a moment after the pointer
//! leaves the link and the panel, or on Escape.
//!
//! One set of listeners on the window serves every link, because the links
//! of rendered markdown are HTML, not components. The modifier is what
//! opens the panel, so that a pointer that moves across a page does not
//! open one panel after another. Plain hover opens it after a short wait.

use aurora_dark::components::{Select, Text};
use aurora_dark::tokens::ApiError;
use leptos::ev;
use leptos::prelude::*;
use std::time::Duration;
use wasm_bindgen::JsCast;
use web_sys::Element;

use super::item::api::{self, Family};
use super::item::markdown::{self, ITEM_PATH};
use crate::auth::use_auth;

/// The links that a preview opens for: the codes in rendered markdown.
const LINK: &str = ".kairos-markdown a[href^='/items/']";
/// The localStorage key of the setting.
const STORAGE_KEY: &str = "kairos.codePeek";
/// The wait before a plain hover opens the panel.
const HOVER_AFTER: Duration = Duration::from_millis(350);
/// The time between the pointer leaving and the panel closing: long
/// enough to cross the gap from the link to the panel.
const CLOSE_AFTER: Duration = Duration::from_millis(250);
const PANEL_WIDTH: f64 = 480.0;
const PANEL_HEIGHT: f64 = 360.0;
const MARGIN: f64 = 8.0;

/// The link under the pointer: the code, and where the link is.
#[derive(Clone, Debug, PartialEq)]
struct Target {
    code: String,
    rect: Rect,
}

/// The box of a link in the viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    left: f64,
    top: f64,
    bottom: f64,
}

/// The code of an item link: `/items/DEMO-T-0001` and the same with a
/// query or a fragment. `None` for any other path, and for a code that no
/// family has.
fn code_of_href(href: &str) -> Option<String> {
    let rest = href.strip_prefix(ITEM_PATH)?;
    let code = rest.split(['?', '#', '/']).next()?;
    Family::of_short_code(code)?;
    Some(code.to_string())
}

/// What opens the panel. The setting of the person, kept in this browser.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PeekMode {
    /// Shift or Ctrl while the pointer is on the link. (Ctrl+click is a
    /// context menu on a Mac, and Shift is free there, so both do.)
    #[default]
    Modifier,
    /// The pointer on the link, after a short wait.
    Hover,
    /// Never.
    Off,
}

impl PeekMode {
    const ALL: [PeekMode; 3] = [PeekMode::Modifier, PeekMode::Hover, PeekMode::Off];

    fn as_str(self) -> &'static str {
        match self {
            PeekMode::Modifier => "modifier",
            PeekMode::Hover => "hover",
            PeekMode::Off => "off",
        }
    }

    fn label(self) -> &'static str {
        match self {
            PeekMode::Modifier => "Preview codes: Shift or Ctrl + hover",
            PeekMode::Hover => "Preview codes: hover",
            PeekMode::Off => "Preview codes: off",
        }
    }

    /// The mode of a stored value; the default for none or an unknown one.
    fn parse(raw: Option<&str>) -> PeekMode {
        PeekMode::ALL
            .into_iter()
            .find(|mode| Some(mode.as_str()) == raw)
            .unwrap_or_default()
    }

    /// Does a pointer on the link, with these keys down, open the panel at
    /// once? (Hover opens after a wait, so it is not here.)
    fn opens_now(self, shift: bool, ctrl: bool) -> bool {
        self == PeekMode::Modifier && (shift || ctrl)
    }

    /// Does a modifier that goes down on a link open the panel?
    fn opens_on_key(self) -> bool {
        self == PeekMode::Modifier
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// The setting, as a signal in the context: read from this browser, and
/// written back on each change. Without storage it holds for this tab.
pub fn provide_mode() {
    let stored = local_storage().and_then(|s| s.get_item(STORAGE_KEY).ok().flatten());
    let mode = RwSignal::new(PeekMode::parse(stored.as_deref()));
    Effect::new(move |_| {
        let mode = mode.get();
        if let Some(storage) = local_storage() {
            let _ = storage.set_item(STORAGE_KEY, mode.as_str());
        }
    });
    provide_context(mode);
}

fn use_mode() -> RwSignal<PeekMode> {
    expect_context::<RwSignal<PeekMode>>()
}

/// The control of the setting, for the top bar.
#[component]
pub fn CodePeekSetting() -> impl IntoView {
    let mode = use_mode();
    let value = RwSignal::new(mode.get_untracked().as_str().to_string());
    Effect::new(move |_| {
        mode.set(PeekMode::parse(Some(&value.get())));
    });
    let pairs: Vec<(String, String)> = PeekMode::ALL
        .into_iter()
        .map(|m| (m.as_str().to_string(), m.label().to_string()))
        .collect();
    view! {
        <span data-testid="code-peek-setting">
            <Select option_pairs=pairs value aria_label="Code preview"/>
        </span>
    }
}

/// The `style` of the panel: below the link when it fits, above it when it
/// does not, and inside the viewport across.
fn placement(rect: Rect, viewport: (f64, f64)) -> String {
    let (width, height) = viewport;
    let max_left = (width - PANEL_WIDTH - MARGIN).max(MARGIN);
    let left = rect.left.clamp(MARGIN, max_left);
    let below = rect.bottom + 4.0;
    if below + PANEL_HEIGHT <= height || rect.top < PANEL_HEIGHT + MARGIN {
        format!("left:{left}px;top:{below}px;")
    } else {
        format!("left:{left}px;bottom:{}px;", height - rect.top + 4.0)
    }
}

/// The item link under `event`, if the target is one.
fn target_of(event: &web_sys::PointerEvent) -> Option<(Element, Target)> {
    let element = event.target()?.dyn_into::<Element>().ok()?;
    let link = element.closest(LINK).ok()??;
    let code = code_of_href(&link.get_attribute("href")?)?;
    let rect = link.get_bounding_client_rect();
    let target = Target {
        code,
        rect: Rect {
            left: rect.left(),
            top: rect.top(),
            bottom: rect.bottom(),
        },
    };
    Some((link, target))
}

fn viewport() -> (f64, f64) {
    let size = |value: Result<wasm_bindgen::JsValue, _>| value.ok().and_then(|v| v.as_f64());
    web_sys::window()
        .map(|w| {
            (
                size(w.inner_width()).unwrap_or(1280.0),
                size(w.inner_height()).unwrap_or(800.0),
            )
        })
        .unwrap_or((1280.0, 800.0))
}

/// The listeners, and the panel. One in the shell.
#[component]
pub fn CodePeek() -> impl IntoView {
    let auth = use_auth();
    let mode = use_mode();
    // The link under the pointer, and the link that the panel is for.
    let hovered = RwSignal::new(None::<Target>);
    let shown = RwSignal::new(None::<Target>);
    let timer = StoredValue::new_local(None::<leptos::leptos_dom::helpers::TimeoutHandle>);
    let open_timer = StoredValue::new_local(None::<leptos::leptos_dom::helpers::TimeoutHandle>);

    let cancel_open = move || {
        open_timer.update_value(|handle| {
            if let Some(handle) = handle.take() {
                handle.clear();
            }
        });
    };
    let cancel_close = move || {
        timer.update_value(|handle| {
            if let Some(handle) = handle.take() {
                handle.clear();
            }
        });
    };
    let close_soon = move || {
        cancel_close();
        let handle = set_timeout_with_handle(move || shown.set(None), CLOSE_AFTER).ok();
        timer.set_value(handle);
    };

    let over = window_event_listener(ev::pointerover, move |event| {
        let Some((_, target)) = target_of(&event) else {
            return;
        };
        cancel_close();
        cancel_open();
        hovered.set(Some(target.clone()));
        let mode = mode.get_untracked();
        if mode.opens_now(event.shift_key(), event.ctrl_key()) {
            shown.set(Some(target));
        } else if mode == PeekMode::Hover {
            let later = target.clone();
            let handle = set_timeout_with_handle(move || shown.set(Some(later)), HOVER_AFTER).ok();
            open_timer.set_value(handle);
        }
    });
    let out = window_event_listener(ev::pointerout, move |event| {
        let Some((link, _)) = target_of(&event) else {
            return;
        };
        // From the link to something inside it is not a leave.
        let inside = event
            .related_target()
            .and_then(|to| to.dyn_into::<web_sys::Node>().ok())
            .is_some_and(|to| link.contains(Some(&to)));
        if !inside {
            hovered.set(None);
            cancel_open();
            close_soon();
        }
    });
    // A modifier that goes down with the pointer already on the link.
    let key = window_event_listener(ev::keydown, move |event| match event.key().as_str() {
        "Shift" | "Control" => {
            if let Some(target) = hovered.get_untracked()
                && mode.get_untracked().opens_on_key()
            {
                cancel_close();
                shown.set(Some(target));
            }
        }
        "Escape" => shown.set(None),
        _ => {}
    });
    Effect::new(move |_| {
        if mode.get() == PeekMode::Off {
            cancel_open();
            shown.set(None);
        }
    });
    on_cleanup(move || {
        over.remove();
        out.remove();
        key.remove();
    });

    let code = Memo::new(move |_| shown.with(|t| t.as_ref().map(|t| t.code.clone())));
    let detail = LocalResource::new(move || {
        let code = code.get();
        async move {
            let code = code?;
            let family = Family::of_short_code(&code)?;
            Some(api::fetch_item(auth, family, code).await)
        }
    });

    view! {
        {move || {
            shown.get().map(|target| {
                let style = placement(target.rect, viewport());
                view! {
                    <div
                        class="kairos-code-peek"
                        style=style
                        data-testid="code-peek"
                        on:pointerenter=move |_| cancel_close()
                        on:pointerleave=move |_| close_soon()
                    >
                        <div class="kairos-code-peek__head">
                            <Text mono=true size="sm">{target.code.clone()}</Text>
                        </div>
                        {move || match detail.get().flatten() {
                            None => view! { <Text dimmed=true size="sm">"Loading…"</Text> }.into_any(),
                            Some(Err(error)) => view! {
                                <Text dimmed=true size="sm">{message(&error)}</Text>
                            }
                            .into_any(),
                            Some(Ok(item)) => {
                                let html = markdown::to_html(&item.content);
                                view! {
                                    <div class="kairos-code-peek__title">{item.title.clone()}</div>
                                    <div class="kairos-markdown kairos-code-peek__body" inner_html=html></div>
                                }
                                .into_any()
                            }
                        }}
                    </div>
                }
            })
        }}
    }
}

/// What the panel says when the read fails.
fn message(error: &ApiError) -> String {
    match error {
        ApiError::Http { status: 404, .. } => "There is no such item.".to_string(),
        _ => "The item did not load.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_of_an_item_link() {
        assert_eq!(
            code_of_href("/items/DEMO-T-0042").as_deref(),
            Some("DEMO-T-0042")
        );
        assert_eq!(
            code_of_href("/items/MY-TEAM-A-0007?retired=X#n-3").as_deref(),
            Some("MY-TEAM-A-0007")
        );
    }

    #[test]
    fn other_links_have_no_code() {
        assert_eq!(code_of_href("/boards/demo"), None);
        assert_eq!(code_of_href("/items/not-a-code"), None);
        assert_eq!(code_of_href("https://example.com/items/DEMO-T-0042"), None);
    }

    #[test]
    fn the_modifier_mode_opens_on_shift_or_ctrl_only() {
        let mode = PeekMode::Modifier;
        assert!(mode.opens_now(true, false));
        assert!(mode.opens_now(false, true));
        assert!(!mode.opens_now(false, false));
        assert!(mode.opens_on_key());
    }

    #[test]
    fn hover_and_off_never_open_at_once() {
        for mode in [PeekMode::Hover, PeekMode::Off] {
            assert!(!mode.opens_now(true, true), "{mode:?}");
            assert!(!mode.opens_on_key(), "{mode:?}");
        }
    }

    #[test]
    fn the_setting_round_trips_and_defaults_to_the_modifier() {
        for mode in PeekMode::ALL {
            assert_eq!(PeekMode::parse(Some(mode.as_str())), mode);
        }
        assert_eq!(PeekMode::parse(None), PeekMode::Modifier);
        assert_eq!(PeekMode::parse(Some("sideways")), PeekMode::Modifier);
    }

    #[test]
    fn the_panel_goes_below_when_it_fits_and_above_when_not() {
        let near_top = Rect {
            left: 100.0,
            top: 40.0,
            bottom: 60.0,
        };
        assert_eq!(placement(near_top, (1280.0, 800.0)), "left:100px;top:64px;");
        let near_bottom = Rect {
            left: 100.0,
            top: 700.0,
            bottom: 720.0,
        };
        assert_eq!(
            placement(near_bottom, (1280.0, 800.0)),
            "left:100px;bottom:104px;"
        );
    }

    #[test]
    fn the_panel_stays_inside_the_viewport_across() {
        let right = Rect {
            left: 1200.0,
            top: 40.0,
            bottom: 60.0,
        };
        assert_eq!(placement(right, (1280.0, 800.0)), "left:792px;top:64px;");
    }
}
