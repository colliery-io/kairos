//! `kairos notifications` (KAIROS-T-0592): the notifications of the caller
//! (`/api/me/notifications`, KAIROS-T-0382). An agent key has its own read
//! state (KAIROS-T-0550).
//!
//! `list` shows each unread notification (newest first, 100 at most), then
//! the 20 newest read ones. `--before <ID>` gives the next page of the
//! unread ones, and `--read-before <ID>` the next page of the read ones.
//! `read` marks notifications read: by id, or each unread one with `--all`.

use kairos_client::types_notifications::{
    MarkNotificationsReadRequest, MarkNotificationsReadResponse, Notification, NotificationList,
};

use crate::context::{Common, client, print_json};
use crate::error::CliError;
use crate::table::Table;

/// The most unread notifications on a page of the server.
const UNREAD_PAGE: usize = 100;
/// The read notifications on the first page of the server.
const READ_FIRST_PAGE: usize = 20;
/// The most read notifications on a `--read-before` page of the server.
const READ_PAGE: usize = 100;

/// Your notifications.
#[derive(clap::Subcommand, Debug)]
pub enum NotificationsCommand {
    /// List your unread notifications (newest first), then the 20 newest
    /// read ones
    List {
        /// The next page of the unread notifications: those older than the
        /// notification with this id
        #[arg(long, value_name = "ID", conflicts_with = "read_before")]
        before: Option<String>,
        /// The next page of the read notifications: those read before the
        /// notification with this id
        #[arg(long, value_name = "ID")]
        read_before: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    /// Mark notifications read: give their ids, or --all for each unread
    /// one
    Read {
        /// The ids of the notifications (`notifications list` shows them)
        #[arg(
            value_name = "ID",
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        ids: Vec<String>,
        /// Mark each unread notification read, up to the newest one now. A
        /// notification that comes later stays unread
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        common: Common,
    },
}

impl NotificationsCommand {
    pub async fn run(self) -> Result<(), CliError> {
        match self {
            Self::List {
                before,
                read_before,
                common,
            } => {
                let client = client(&common)?;
                let page = Page::of(before.as_deref(), read_before.as_deref());
                let list = match (&before, &read_before) {
                    (Some(before), _) => client.notifications_before(before).await?,
                    (None, Some(read_before)) => {
                        client.notifications_read_before(read_before).await?
                    }
                    (None, None) => client.notifications().await?,
                };
                if common.json {
                    return print_json(&serde_json::json!({
                        "items": list.items,
                        "total": list.items.len(),
                        "next_offset": null,
                        "unread": list.unread,
                    }));
                }
                print!("{}", list_text(&list, page));
                Ok(())
            }
            Self::Read { ids, all, common } => {
                let client = client(&common)?;
                let request = if all {
                    let list = client.notifications().await?;
                    let Some(newest) = list.items.iter().find(|n| n.read_at.is_none()) else {
                        if common.json {
                            return print_json(&MarkNotificationsReadResponse {
                                marked: Vec::new(),
                                unread: 0,
                            });
                        }
                        println!("You have no unread notification.");
                        return Ok(());
                    };
                    MarkNotificationsReadRequest {
                        ids: None,
                        all: Some(true),
                        up_to: Some(newest.id.clone()),
                    }
                } else {
                    MarkNotificationsReadRequest {
                        ids: Some(ids),
                        all: None,
                        up_to: None,
                    }
                };
                let response = client.mark_notifications_read(&request).await?;
                if common.json {
                    return print_json(&response);
                }
                println!("{}", marked_line(&response));
                Ok(())
            }
        }
    }
}

/// The page that `list` asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    First,
    Unread,
    Read,
}

impl Page {
    fn of(before: Option<&str>, read_before: Option<&str>) -> Self {
        match (before, read_before) {
            (Some(_), _) => Self::Unread,
            (None, Some(_)) => Self::Read,
            (None, None) => Self::First,
        }
    }
}

/// The text of `list`: the count of the unread ones, a table, and the
/// flags of the next pages. Pure.
fn list_text(list: &NotificationList, page: Page) -> String {
    let mut out = format!(
        "You have {} unread notification{}.\n",
        list.unread,
        if list.unread == 1 { "" } else { "s" }
    );
    if list.items.is_empty() {
        out.push_str("(none)\n");
        return out;
    }
    let mut table = Table::new(&["ID", "KIND", "ITEM", "WHEN", "READ", "TEXT"]);
    for notification in &list.items {
        table.row(vec![
            notification.id.clone(),
            notification.kind.clone(),
            notification
                .short_code
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            notification.created_at.clone(),
            if notification.read_at.is_some() {
                "read"
            } else {
                "unread"
            }
            .to_string(),
            notification.text.clone(),
        ]);
    }
    out.push_str(&table.render());
    let unread: Vec<&Notification> = list.items.iter().filter(|n| n.read_at.is_none()).collect();
    let read: Vec<&Notification> = list.items.iter().filter(|n| n.read_at.is_some()).collect();
    let more_unread = match page {
        Page::First => usize::try_from(list.unread).unwrap_or(0) > unread.len(),
        Page::Unread => unread.len() >= UNREAD_PAGE,
        Page::Read => false,
    };
    if let (true, Some(last)) = (more_unread, unread.last()) {
        out.push_str(&format!("Older unread: --before {}\n", last.id));
    }
    let more_read = match page {
        Page::First => read.len() >= READ_FIRST_PAGE,
        Page::Read => read.len() >= READ_PAGE,
        Page::Unread => false,
    };
    if let (true, Some(last)) = (more_read, read.last()) {
        out.push_str(&format!("Older read: --read-before {}\n", last.id));
    }
    out
}

/// The line of `read`. Pure.
fn marked_line(response: &MarkNotificationsReadResponse) -> String {
    let marked = response.marked.len();
    format!(
        "Kairos marked {marked} notification{} read. You have {} unread now.",
        if marked == 1 { "" } else { "s" },
        response.unread
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: &str, read: bool) -> Notification {
        Notification {
            id: id.into(),
            kind: "hand_off".into(),
            short_code: Some("ACME-T-0001".into()),
            entity_type: Some("task".into()),
            title: Some("Fix".into()),
            text: "Robin handed off ACME-T-0001 to you.".into(),
            actor_id: None,
            created_at: "2026-10-18T10:00:00Z".into(),
            read_at: read.then(|| "2026-10-18T11:00:00Z".to_string()),
            agent_key_id: None,
            agent_key_name: None,
        }
    }

    #[test]
    fn the_list_counts_the_unread_and_names_the_next_page() {
        let list = NotificationList {
            unread: 3,
            items: vec![notification("n-1", false), notification("n-2", true)],
        };
        let text = list_text(&list, Page::First);
        assert!(
            text.starts_with("You have 3 unread notifications.\n"),
            "{text}"
        );
        assert!(
            text.contains("n-1  hand_off  ACME-T-0001  2026-10-18T10:00:00Z  unread  Robin"),
            "{text}"
        );
        assert!(text.contains("  read    Robin"), "{text}");
        assert!(text.ends_with("Older unread: --before n-1\n"), "{text}");

        let empty = NotificationList {
            unread: 0,
            items: vec![],
        };
        assert_eq!(
            list_text(&empty, Page::First),
            "You have 0 unread notifications.\n(none)\n"
        );
        assert_eq!(Page::of(Some("a"), None), Page::Unread);
        assert_eq!(Page::of(None, Some("a")), Page::Read);
    }

    #[test]
    fn read_says_how_many_changed() {
        let response = MarkNotificationsReadResponse {
            marked: vec!["n-1".into()],
            unread: 2,
        };
        assert_eq!(
            marked_line(&response),
            "Kairos marked 1 notification read. You have 2 unread now."
        );
    }

    #[test]
    fn read_takes_ids_or_all_and_not_both() {
        use clap::Parser;
        #[derive(Parser, Debug)]
        struct Cli {
            #[command(subcommand)]
            command: NotificationsCommand,
        }
        assert!(Cli::try_parse_from(["n", "read", "a", "b"]).is_ok());
        assert!(Cli::try_parse_from(["n", "read", "--all"]).is_ok());
        assert!(Cli::try_parse_from(["n", "read"]).is_err());
        assert!(Cli::try_parse_from(["n", "read", "a", "--all"]).is_err());
        assert!(Cli::try_parse_from(["n", "list", "--before", "a", "--read-before", "b"]).is_err());
    }
}
