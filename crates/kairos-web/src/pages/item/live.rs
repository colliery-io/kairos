//! The live item page (KAIROS-T-0578): the pure rules. The page has one
//! socket on the tenant stream. An event of its code says what to read:
//! the item, the metadata values, or the forge links ([`reads_for`]).
//! Events in one window give one read ([`Reads::add`]).
//!
//! The page does not mount again and does not take the new copy by
//! itself. It compares the copy that it read with the copy that it shows
//! ([`is_newer_copy`], [`values_differ`]). A difference shows the banner
//! "Changed on the server" with the parts that changed ([`changed_parts`],
//! [`banner_text`]). Reload gives the copy to the page, and the editor
//! keeps a dirty draft (KAIROS-T-0618). A change that the person made on
//! this page gives an equal copy, so it shows no banner.
//!
//! The precise kinds of KAIROS-A-0049 come in addition to the coarse kind
//! of the same write. Both fall in one window, so they give one read.

use super::api::{ItemDetail, MetadataValue};
use crate::pages::boards::data::{BOARD_CONFIGURED, ThinEvent};

/// The time that the page waits after the first event, for the other
/// events of the same write (milliseconds).
pub(super) const LIVE_WINDOW_MS: u64 = 250;

/// The kinds that change the metadata values of the item.
const METADATA_CHANGED: &str = "metadata_changed";
/// The kinds that change the forge links of the item.
const LINKS_CHANGED: &str = "item_links_changed";

/// What the page reads after events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Reads {
    pub item: bool,
    pub metadata: bool,
    pub links: bool,
}

impl Reads {
    /// Read everything: a reconnect can have missed any event.
    const ALL: Reads = Reads {
        item: true,
        metadata: true,
        links: true,
    };

    pub fn any(self) -> bool {
        self.item || self.metadata || self.links
    }

    /// Add the reads of one more event of the window.
    pub fn add(&mut self, other: Reads) {
        self.item |= other.item;
        self.metadata |= other.metadata;
        self.links |= other.links;
    }
}

/// What one event asks the page of `code` to read. `None` is a reconnect:
/// read everything. An event of a different code, a change of a board
/// configuration and an event with no item read nothing. A kind that the
/// page does not know reads the item (the contract of the events). Pure,
/// host-tested.
pub(super) fn reads_for(event: Option<&ThinEvent>, code: &str) -> Reads {
    let Some(event) = event else {
        return Reads::ALL;
    };
    if event.short_code.is_empty() || event.short_code != code || event.event == BOARD_CONFIGURED {
        return Reads::default();
    }
    match event.event.as_str() {
        METADATA_CHANGED => Reads {
            metadata: true,
            ..Reads::default()
        },
        LINKS_CHANGED => Reads {
            links: true,
            ..Reads::default()
        },
        // item_updated, item_transitioned, item_moved, claim_changed,
        // task_state_changed, task_cancelled, item_deleted, item_restored,
        // relationship_changed, note_appended, note_promoted, and each kind
        // that comes later.
        _ => Reads {
            item: true,
            ..Reads::default()
        },
    }
}

/// Is the copy that the page read a change of the copy that it shows? Not
/// when it is equal, and not when it is older (a slow read). A claim or a
/// state does not change the version, so a copy of the same version that
/// differs is a change. Pure, host-tested.
pub(super) fn is_newer_copy(shown: &ItemDetail, read: &ItemDetail) -> bool {
    read.version >= shown.version && read != shown
}

/// Is `read` the copy `shown` with new notes and the time of their write
/// (KAIROS-T-0662)? A note keeps the version and moves `updated_at`, so the
/// page takes that time with the notes, and shows no banner. Pure,
/// host-tested.
pub(super) fn only_notes_changed(shown: &ItemDetail, read: &ItemDetail) -> bool {
    let mut probe = shown.clone();
    probe.notes.clone_from(&read.notes);
    probe.updated_at.clone_from(&read.updated_at);
    probe == *read
}

/// Do the metadata values that the page read differ from the values that
/// the panel shows? `None` for the panel: it did not load yet, so there is
/// nothing to compare. The order of the values is not part of the
/// answer. Pure, host-tested.
pub(super) fn values_differ(
    shown: Option<&[MetadataValue]>,
    read: Option<&[MetadataValue]>,
) -> bool {
    let (Some(shown), Some(read)) = (shown, read) else {
        return false;
    };
    let sorted = |values: &[MetadataValue]| {
        let mut pairs: Vec<(String, String)> = values
            .iter()
            .map(|value| (value.slug.clone(), value.value.clone()))
            .collect();
        pairs.sort();
        pairs
    };
    sorted(shown) != sorted(read)
}

/// The parts of the item that changed between the two copies, in the
/// words of the page. Pure, host-tested.
pub(super) fn changed_parts(shown: &ItemDetail, read: &ItemDetail) -> Vec<&'static str> {
    let mut parts = Vec::new();
    let mut note = |changed: bool, part: &'static str| {
        if changed {
            parts.push(part);
        }
    };
    note(read.short_code != shown.short_code, "the code");
    note(
        read.title != shown.title || read.content != shown.content,
        "the content",
    );
    note(
        read.board_id != shown.board_id || read.column_id != shown.column_id,
        "the column",
    );
    note(read.work_class != shown.work_class, "the lane");
    note(read.repository != shown.repository, "the repository");
    note(read.claim != shown.claim, "the claim");
    note(read.state != shown.state, "the state");
    note(read.cancellation != shown.cancellation, "the cancel mark");
    note(read.owner != shown.owner, "the owner");
    note(read.impacts != shown.impacts, "the impacts");
    note(read.lifecycle != shown.lifecycle, "the lifecycle");
    note(read.archived_at != shown.archived_at, "the archive state");
    note(
        read.section_check != shown.section_check,
        "the section check",
    );
    // KAIROS-T-0630: a note is not part of the content.
    note(read.notes != shown.notes, "the notes");
    parts
}

/// The text of the banner. Pure, host-tested.
pub(super) fn banner_text(parts: &[&str]) -> String {
    let list = match parts {
        [] => return "Someone changed this item after the page loaded.".to_string(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    format!("Someone changed {list} after the page loaded.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, code: &str) -> ThinEvent {
        ThinEvent {
            event: kind.to_string(),
            entity_type: "task".to_string(),
            short_code: code.to_string(),
            board_id: Some("b".to_string()),
        }
    }

    const ITEM: Reads = Reads {
        item: true,
        metadata: false,
        links: false,
    };

    /// The kinds of KAIROS-A-0049 and the coarse kinds of the code read the
    /// item; a reconnect reads everything.
    #[test]
    fn an_event_of_the_code_reads_the_item() {
        for kind in [
            "item_updated",
            "item_transitioned",
            "item_moved",
            "claim_changed",
            "task_state_changed",
            "task_cancelled",
            "item_deleted",
            "item_restored",
            "relationship_changed",
            "note_appended",
            "note_promoted",
            "a_kind_from_a_newer_server",
        ] {
            assert_eq!(
                reads_for(Some(&event(kind, "A-T-1")), "A-T-1"),
                ITEM,
                "{kind}"
            );
        }
        assert_eq!(reads_for(None, "A-T-1"), Reads::ALL);
    }

    /// The metadata and the links have their own reads.
    #[test]
    fn metadata_and_links_have_their_own_reads() {
        let metadata = reads_for(Some(&event("metadata_changed", "A-T-1")), "A-T-1");
        assert!(metadata.metadata && !metadata.item && !metadata.links);
        let links = reads_for(Some(&event("item_links_changed", "A-T-1")), "A-T-1");
        assert!(links.links && !links.item && !links.metadata);
    }

    /// An event of a different code, of a board configuration, or with no
    /// code reads nothing.
    #[test]
    fn other_events_read_nothing() {
        let nothing = Reads::default();
        assert_eq!(
            reads_for(Some(&event("item_updated", "A-T-2")), "A-T-1"),
            nothing
        );
        assert_eq!(
            reads_for(Some(&event("board_configured", "A-T-1")), "A-T-1"),
            nothing
        );
        assert_eq!(
            reads_for(Some(&event("notifications_read", "")), ""),
            nothing
        );
        assert!(!nothing.any());
    }

    /// The coarse and the precise kind of one write give one read.
    #[test]
    fn a_window_adds_its_reads() {
        let mut window = Reads::default();
        window.add(reads_for(Some(&event("item_updated", "A-T-1")), "A-T-1"));
        window.add(reads_for(Some(&event("claim_changed", "A-T-1")), "A-T-1"));
        assert_eq!(window, ITEM);
        window.add(reads_for(
            Some(&event("metadata_changed", "A-T-1")),
            "A-T-1",
        ));
        assert!(window.item && window.metadata && !window.links);
    }

    fn item(version: i32) -> ItemDetail {
        serde_json::from_value(serde_json::json!({
            "short_code": "A-T-1",
            "title": "t",
            "content": "c",
            "version": version,
            "id": "i",
            "updated_at": "2026-10-17T00:00:00Z",
        }))
        .expect("item decodes")
    }

    /// An equal copy (the change of this person) and an older copy show no
    /// banner; a claim of the same version does.
    #[test]
    fn only_a_different_copy_that_is_not_older_is_a_change() {
        let shown = item(4);
        assert!(!is_newer_copy(&shown, &item(4)));
        assert!(!is_newer_copy(&shown, &item(3)));
        let mut claimed = item(4);
        claimed.claim = serde_json::from_value(serde_json::json!({
            "user_id": "u",
            "display_name": "Robin",
            "claimed_at": "2026-10-17T00:00:00Z",
        }))
        .ok();
        assert!(claimed.claim.is_some(), "the claim fixture decodes");
        assert!(is_newer_copy(&shown, &claimed));
        let mut edited = item(5);
        edited.content = "theirs".to_string();
        assert!(is_newer_copy(&shown, &edited));
    }

    /// The banner names what changed.
    #[test]
    fn the_banner_names_the_parts() {
        let shown = item(4);
        let mut read = item(5);
        read.content = "theirs".to_string();
        read.column_id = Some("done".to_string());
        assert_eq!(
            changed_parts(&shown, &read),
            vec!["the content", "the column"]
        );
        assert_eq!(
            banner_text(&changed_parts(&shown, &read)),
            "Someone changed the content and the column after the page loaded."
        );
        assert_eq!(
            banner_text(&["the claim", "the state", "the metadata"]),
            "Someone changed the claim, the state and the metadata after the page loaded."
        );
        assert_eq!(
            banner_text(&[]),
            "Someone changed this item after the page loaded."
        );
        let mut moved = item(5);
        moved.short_code = "B-T-9".to_string();
        assert_eq!(changed_parts(&shown, &moved), vec!["the code"]);
        // KAIROS-T-0630: a new note of the same version is a change of the
        // notes.
        let noted: ItemDetail = serde_json::from_value(serde_json::json!({
            "short_code": "A-T-1",
            "title": "t",
            "content": "c",
            "version": 4,
            "id": "i",
            "updated_at": "2026-10-17T00:00:00Z",
            "notes": [{"id": "n-1", "author": "Robin", "created_at": "2026-10-17T00:00:00Z", "text": "One."}],
        }))
        .expect("item decodes");
        assert!(is_newer_copy(&shown, &noted));
        assert_eq!(changed_parts(&shown, &noted), vec!["the notes"]);
    }

    /// KAIROS-T-0662: a note moves `updated_at` and keeps the version. The
    /// page takes the notes and the time; a change of another part is not
    /// only the notes.
    #[test]
    fn a_note_and_its_time_are_only_the_notes() {
        let shown = item(4);
        let mut noted = item(4);
        noted.updated_at = "2026-10-18T00:00:00Z".to_string();
        noted.notes = serde_json::from_value(serde_json::json!([
            {"id": "n-1", "author": "Robin", "created_at": "2026-10-18T00:00:00Z", "text": "One."}
        ]))
        .ok();
        assert!(noted.notes.is_some(), "the notes fixture decodes");
        assert!(only_notes_changed(&shown, &noted));
        let mut edited = noted.clone();
        edited.version = 5;
        assert!(!only_notes_changed(&shown, &edited));
    }

    fn value(slug: &str, value: &str) -> MetadataValue {
        MetadataValue {
            slug: slug.to_string(),
            value: value.to_string(),
        }
    }

    /// The metadata values differ by content, not by order; nothing to
    /// compare before the panel loads.
    #[test]
    fn metadata_values_differ_by_content() {
        let shown = vec![value("priority", "high"), value("size", "s")];
        let reordered = vec![value("size", "s"), value("priority", "high")];
        let changed = vec![value("priority", "low"), value("size", "s")];
        assert!(!values_differ(Some(&shown), Some(&reordered)));
        assert!(values_differ(Some(&shown), Some(&changed)));
        assert!(values_differ(Some(&shown), Some(&shown[..1])));
        assert!(!values_differ(None, Some(&changed)));
        assert!(!values_differ(Some(&shown), None));
    }
}
