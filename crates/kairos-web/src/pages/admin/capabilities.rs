//! The KAIROS-A-0006 capability vocabulary, presented sanely (T-0043 AC).
//!
//! # Presentation design (decision, KAIROS-T-0043)
//!
//! The grant editor is toggles over the fixed vocabulary, grouped the way
//! A-0006 groups it, with the glob forms as explicit presets rather than
//! free text:
//!
//! - **Full access** — one switch mapping to `*` (every capability on the
//!   board); when on, nothing else needs choosing and the groups collapse
//!   to a note.
//! - **Content** — a family switch for `manage_*` plus per-type switches
//!   (`manage_strategies` … `manage_adrs`). Globs match by prefix
//!   (SQL `LIKE`), so `manage_*` also covers `administer_members`; the family
//!   switch says so and the Administration group shows "covered".
//! - **Workflow** — `transition_items` (the only member of `transition_*`
//!   today; the editor emits the concrete capability, and normalizes a
//!   stored `transition_*` grant to it on load).
//! - **Configuration** — a family switch for `configure_*` plus
//!   `configure_boards`. KAIROS-T-0182 removed `configure_templates` and
//!   `configure_metadata` from the vocabulary — they authorised nothing, and
//!   could not, being tenant-wide resources behind a board-scoped grant.
//! - **Administration** — `administer_members` (add/remove members, grant
//!   capabilities).
//!
//! The grant sent to the server is exactly the enabled set: globs travel
//! as globs (one `manage_*` row, not five rows), and singles already
//! covered by an enabled family glob are dropped rather than duplicated.
//! Pure set logic lives in [`compose_selection`] / [`SelectionFlags`] and
//! is host-unit-tested; the component is a thin binding over it.

use aurora_dark::components::{Code, Divider, Group, Pill, Stack, Switch, Text};
use aurora_dark::tokens::token;
use leptos::prelude::*;

/// Everything the editor can emit besides the globs.
const SINGLES: &[&str] = &[
    "manage_strategies",
    "manage_initiatives",
    "manage_tasks",
    "manage_documents",
    "manage_adrs",
    "transition_items",
    "configure_boards",
    "administer_members",
];

/// The editor's state as plain booleans — the pure, testable core.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionFlags {
    pub full_access: bool,
    pub manage_all: bool,
    pub configure_all: bool,
    /// Enabled concrete capabilities, in `SINGLES` order.
    pub singles: Vec<String>,
}

impl SelectionFlags {
    /// Parse a stored grant set (what `GET .../members` returns) into
    /// editor flags. Unknown values are ignored (the API validates the
    /// vocabulary on write); a stored `transition_*` normalizes to
    /// `transition_items` (its only member today).
    pub fn from_capabilities(capabilities: &[String]) -> Self {
        let has = |name: &str| capabilities.iter().any(|c| c == name);
        Self {
            full_access: has("*"),
            manage_all: has("manage_*"),
            configure_all: has("configure_*"),
            singles: SINGLES
                .iter()
                .filter(|&&single| {
                    has(single) || (single == "transition_items" && has("transition_*"))
                })
                .map(|&single| single.to_string())
                .collect(),
        }
    }
}

/// Flags → the capability list the API receives: `*` alone wins; family
/// globs travel as globs; singles already covered by an enabled glob are
/// dropped (no redundant rows).
pub fn compose_selection(flags: &SelectionFlags) -> Vec<String> {
    if flags.full_access {
        return vec!["*".to_string()];
    }
    let mut selection = Vec::new();
    if flags.manage_all {
        selection.push("manage_*".to_string());
    }
    if flags.configure_all {
        selection.push("configure_*".to_string());
    }
    for single in &flags.singles {
        let covered = (flags.manage_all && single.starts_with("manage_"))
            || (flags.configure_all && single.starts_with("configure_"));
        if !covered {
            selection.push(single.clone());
        }
    }
    selection
}

/// Reactive editor state: one signal per toggle (signals are Copy, so the
/// struct clones freely into row closures).
#[derive(Clone, Copy)]
pub struct EditorState {
    pub full_access: RwSignal<bool>,
    pub manage_all: RwSignal<bool>,
    pub configure_all: RwSignal<bool>,
    pub manage_strategies: RwSignal<bool>,
    pub manage_initiatives: RwSignal<bool>,
    pub manage_tasks: RwSignal<bool>,
    pub manage_documents: RwSignal<bool>,
    pub manage_adrs: RwSignal<bool>,
    pub transition_items: RwSignal<bool>,
    pub configure_boards: RwSignal<bool>,
    pub administer_members: RwSignal<bool>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorState {
    /// Fresh editor, everything off (whitelist model: no grant by default).
    pub fn new() -> Self {
        Self::from_capabilities(&[])
    }

    /// Editor prefilled from a member's stored grants.
    pub fn from_capabilities(capabilities: &[String]) -> Self {
        let flags = SelectionFlags::from_capabilities(capabilities);
        let single = |name: &str| RwSignal::new(flags.singles.iter().any(|s| s == name));
        Self {
            full_access: RwSignal::new(flags.full_access),
            manage_all: RwSignal::new(flags.manage_all),
            configure_all: RwSignal::new(flags.configure_all),
            manage_strategies: single("manage_strategies"),
            manage_initiatives: single("manage_initiatives"),
            manage_tasks: single("manage_tasks"),
            manage_documents: single("manage_documents"),
            manage_adrs: single("manage_adrs"),
            transition_items: single("transition_items"),
            configure_boards: single("configure_boards"),
            administer_members: single("administer_members"),
        }
    }

    fn single_signal(&self, name: &str) -> RwSignal<bool> {
        match name {
            "manage_strategies" => self.manage_strategies,
            "manage_initiatives" => self.manage_initiatives,
            "manage_tasks" => self.manage_tasks,
            "manage_documents" => self.manage_documents,
            "manage_adrs" => self.manage_adrs,
            "transition_items" => self.transition_items,
            "configure_boards" => self.configure_boards,
            _ => self.administer_members,
        }
    }

    /// Snapshot the toggles into pure flags (untracked — call at submit).
    pub fn flags(&self) -> SelectionFlags {
        SelectionFlags {
            full_access: self.full_access.get_untracked(),
            manage_all: self.manage_all.get_untracked(),
            configure_all: self.configure_all.get_untracked(),
            singles: SINGLES
                .iter()
                .filter(|&&name| self.single_signal(name).get_untracked())
                .map(|&name| name.to_string())
                .collect(),
        }
    }

    /// The capability list to send (empty means "nothing selected" — the
    /// API rejects empty grants, so callers surface that before sending).
    pub fn selection(&self) -> Vec<String> {
        compose_selection(&self.flags())
    }
}

/// One capability toggle row: switch + human label + the raw capability
/// name (mono), so what gets granted is never a mystery.
#[component]
fn CapabilityRow(
    checked: RwSignal<bool>,
    #[prop(into)] label: String,
    #[prop(into)] name: String,
) -> impl IntoView {
    view! {
        <Group gap="sm">
            <Switch checked label/>
            <Code>{name}</Code>
        </Group>
    }
}

/// The grant editor (see module docs for the presentation design).
#[component]
pub fn CapabilityEditor(state: EditorState) -> impl IntoView {
    view! {
        <Stack gap="sm">
            <Group gap="sm">
                <Switch checked=state.full_access label="Full access"/>
                <Code>"*"</Code>
                <Text dimmed=true size="xs">"every capability on this board"</Text>
            </Group>
            <Show
                when=move || !state.full_access.get()
                fallback=|| view! {
                    <Text dimmed=true size="sm">
                        "Full access covers everything below — one \"*\" grant."
                    </Text>
                }
            >
                <Stack gap="sm">
                    <Divider/>
                    <Text bright=true size="sm" bold=true>"Content"</Text>
                    <Group gap="sm">
                        <Switch checked=state.manage_all label="All manage capabilities"/>
                        <Code>"manage_*"</Code>
                        <Text dimmed=true size="xs">
                            "prefix glob: every content type, plus board member \
                             administration (administer_members)"
                        </Text>
                    </Group>
                    <Show
                        when=move || !state.manage_all.get()
                        fallback=|| view! {
                            <Text dimmed=true size="xs">
                                "Individual content grants covered by manage_*."
                            </Text>
                        }
                    >
                        <Stack gap="xs">
                            <CapabilityRow checked=state.manage_strategies
                                label="Strategies" name="manage_strategies"/>
                            <CapabilityRow checked=state.manage_initiatives
                                label="Initiatives" name="manage_initiatives"/>
                            <CapabilityRow checked=state.manage_tasks
                                label="Tasks" name="manage_tasks"/>
                            <CapabilityRow checked=state.manage_documents
                                label="Documents" name="manage_documents"/>
                            <CapabilityRow checked=state.manage_adrs
                                label="ADRs" name="manage_adrs"/>
                        </Stack>
                    </Show>
                    <Divider/>
                    <Text bright=true size="sm" bold=true>"Workflow"</Text>
                    <CapabilityRow checked=state.transition_items
                        label="Move items between columns" name="transition_items"/>
                    <Divider/>
                    <Text bright=true size="sm" bold=true>"Configuration"</Text>
                    <Group gap="sm">
                        <Switch checked=state.configure_all label="All configuration"/>
                        <Code>"configure_*"</Code>
                        <Text dimmed=true size="xs">
                            "boards, templates, metadata definitions"
                        </Text>
                    </Group>
                    <Show
                        when=move || !state.configure_all.get()
                        fallback=|| view! {
                            <Text dimmed=true size="xs">
                                "Individual configuration grants covered by configure_*."
                            </Text>
                        }
                    >
                        <Stack gap="xs">
                            <CapabilityRow checked=state.configure_boards
                                label="Board configuration" name="configure_boards"/>
                        </Stack>
                    </Show>
                    <Divider/>
                    <Text bright=true size="sm" bold=true>"Administration"</Text>
                    <Show
                        when=move || !state.manage_all.get()
                        fallback=|| view! {
                            <Text dimmed=true size="xs">
                                "Member administration covered by manage_* (prefix glob)."
                            </Text>
                        }
                    >
                        <CapabilityRow checked=state.administer_members
                            label="Members: add/remove, grant capabilities"
                            name="administer_members"/>
                    </Show>
                </Stack>
            </Show>
        </Stack>
    }
}

/// A member's stored grants as pills: `*` gold, family globs violet,
/// concrete capabilities ice — scannable in the member list.
#[component]
pub fn CapabilityPills(capabilities: Vec<String>) -> impl IntoView {
    view! {
        <Group gap="xs" wrap=true>
            {capabilities
                .into_iter()
                .map(|capability| {
                    let color = if capability == "*" {
                        token::GOLD
                    } else if capability.ends_with('*') {
                        token::VIOLET
                    } else {
                        token::ICE
                    };
                    view! { <Pill color=color>{capability}</Pill> }
                })
                .collect_view()}
        </Group>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(list: &[&str]) -> Vec<String> {
        list.iter().map(|c| c.to_string()).collect()
    }

    /// Full access always composes to exactly `["*"]`.
    #[test]
    fn full_access_wins() {
        let mut flags = SelectionFlags::from_capabilities(&caps(&["*", "manage_tasks"]));
        assert!(flags.full_access);
        flags.singles = vec!["manage_tasks".into()];
        assert_eq!(compose_selection(&flags), vec!["*"]);
    }

    /// Family globs travel as globs and swallow their covered singles.
    ///
    /// `administer_members` is deliberately in this fixture and deliberately
    /// SURVIVES (KAIROS-T-0183). While it was called `manage_members` it was
    /// swallowed here, because `compose_selection` tests the literal prefix —
    /// so an admin who ticked board administration and the `manage_*` family
    /// sent one `manage_*` row, and got board administration whether they meant
    /// it or not. The rename fixes this without touching the collapsing logic:
    /// `administer_members` simply does not start with `manage_`.
    #[test]
    fn family_glob_swallows_covered_singles() {
        let flags = SelectionFlags {
            full_access: false,
            manage_all: true,
            configure_all: false,
            singles: caps(&[
                "manage_tasks",
                "administer_members",
                "transition_items",
                "configure_boards",
            ]),
        };
        assert_eq!(
            compose_selection(&flags),
            caps(&[
                "manage_*",
                "administer_members",
                "transition_items",
                "configure_boards"
            ])
        );
    }

    /// Singles round-trip: stored grants → flags → the same set out.
    #[test]
    fn singles_round_trip() {
        let stored = caps(&["manage_tasks", "transition_items"]);
        let flags = SelectionFlags::from_capabilities(&stored);
        assert!(!flags.full_access && !flags.manage_all && !flags.configure_all);
        assert_eq!(compose_selection(&flags), stored);
    }

    /// A stored `transition_*` glob normalizes to its only member.
    #[test]
    fn transition_glob_normalizes() {
        let flags = SelectionFlags::from_capabilities(&caps(&["transition_*"]));
        assert_eq!(compose_selection(&flags), caps(&["transition_items"]));
    }

    /// Nothing selected composes to the empty list (callers block the
    /// submit — the API rejects empty grants with 422).
    #[test]
    fn empty_selection_is_empty() {
        assert!(compose_selection(&SelectionFlags::default()).is_empty());
    }
}
