//! The five S-0005 entity families as CLI nouns (KAIROS-T-0037 /
//! KAIROS-A-0015): `strategies|initiatives|tasks|documents|adrs`, each with
//! `list/get/create/edit/transition/delete` (documents have no transition
//! endpoint). A thin veneer over `kairos-client` — the only CLI-side logic
//! is flag plumbing, the fetch-then-patch edit flow, and rendering.

use std::io::Read;
use std::path::{Path, PathBuf};

use clap::Args;

use kairos_client::types::{
    Adr, AppendNoteResponse, CreateAdrRequest, CreateDocumentRequest, CreateInitiativeRequest,
    CreateStrategyRequest, CreateTaskRequest, DeleteResponse, Document, ImpactListQuery,
    Initiative, ItemNote, ListEnvelope, ListQuery, Pagination, PromoteNoteRequest,
    PromoteNoteResponse, RestoreResponse, SetTaskReviewRequest, Strategy, Task,
    UpdateContentRequest,
};
use kairos_client::types_meta::{
    HistoryVersion, ItemActivityEntry, ItemMetadataResponse, ItemRelationshipsResponse,
    MetadataValue, RelatedItem, SetParentResponse, UpdateMetadataRequest,
};
use kairos_client::types_repositories::Impact;
use kairos_client::{EntityKind, KairosClient};

use crate::commands::links::reference_text;
use crate::context::{
    Common, client, column_id, find_column, is_uuid, print_json, team_id, template_id,
};
use crate::error::CliError;
use crate::listing::{Listing, Paging, fetch};
use crate::table::Table;

// ---------------------------------------------------------------------------
// Shared verb arguments
// ---------------------------------------------------------------------------

/// `?limit=&offset=` pagination flags for the `list` verbs.
#[derive(Args, Debug)]
pub struct ListArgs {
    /// Page size (server default 50, max 200)
    #[arg(long)]
    pub limit: Option<i64>,
    /// Rows to skip
    #[arg(long)]
    pub offset: Option<i64>,
    /// Read each page: the CLI asks for one page after the other, and
    /// prints each row. Not with --limit or --offset
    #[arg(long, conflicts_with_all = ["limit", "offset"])]
    pub all: bool,
    #[command(flatten)]
    pub common: Common,
}

impl ListArgs {
    /// The page flags (KAIROS-T-0644).
    pub fn paging(&self) -> Paging {
        Paging {
            limit: self.limit,
            offset: self.offset,
            all: self.all,
        }
    }
}

// Deliberately NOT [`ListArgs`], which the org nouns (boards, teams,
// members, streams, tenants) share and which have no archived mode. A flag
// on the shared struct would appear on those verbs too and do nothing —
// worse than absent, because the reader would believe they had asked.
//
// The doc comment below is user-facing: clap renders it as this verb's
// long help.
/// `?limit=&offset=&include_deleted=` flags for the entity `list` verbs.
#[derive(Args, Debug)]
pub struct EntityListArgs {
    /// Page size (server default 50, max 200)
    #[arg(long)]
    pub limit: Option<i64>,
    /// Rows to skip
    #[arg(long)]
    pub offset: Option<i64>,
    /// Also list archived (put-away) work, marked `[archived]` in the
    /// CODE column. Default: live work only
    #[arg(long)]
    pub include_deleted: bool,
    /// Read each page: the CLI asks for one page after the other, and
    /// prints each row. Not with --limit or --offset
    #[arg(long, conflicts_with_all = ["limit", "offset"])]
    pub all: bool,
    #[command(flatten)]
    pub common: Common,
}

impl EntityListArgs {
    /// The page flags (KAIROS-T-0644).
    pub fn paging(&self) -> Paging {
        Paging {
            limit: self.limit,
            offset: self.offset,
            all: self.all,
        }
    }

    /// The query of one page.
    pub fn query(&self, page: Pagination) -> ListQuery {
        ListQuery {
            limit: page.limit,
            offset: page.offset,
            include_deleted: self.include_deleted,
        }
    }
}

// COLLIERY-T-0269. Not [`EntityListArgs`], for the reason that
// [`EntityListArgs`] is not [`ListArgs`]: a strategy, an initiative and a
// task impact no repository, and the flag would do nothing there.
/// `?limit=&offset=&include_deleted=&repository=` flags for the `list`
/// verb of documents and of ADRs.
#[derive(Args, Debug)]
pub struct ImpactListArgs {
    /// Page size (server default 50, max 200)
    #[arg(long)]
    pub limit: Option<i64>,
    /// Rows to skip
    #[arg(long)]
    pub offset: Option<i64>,
    /// Also list archived (put-away) work, marked `[archived]` in the
    /// CODE column. Default: live work only
    #[arg(long)]
    pub include_deleted: bool,
    /// Only the items that impact this repository (slug or UUID)
    #[arg(long, value_name = "REPOSITORY")]
    pub repo: Option<String>,
    /// Read each page: the CLI asks for one page after the other, and
    /// prints each row. Not with --limit or --offset
    #[arg(long, conflicts_with_all = ["limit", "offset"])]
    pub all: bool,
    #[command(flatten)]
    pub common: Common,
}

impl ImpactListArgs {
    /// The page flags (KAIROS-T-0644).
    pub fn paging(&self) -> Paging {
        Paging {
            limit: self.limit,
            offset: self.offset,
            all: self.all,
        }
    }

    /// The query of one page.
    pub fn query(&self, page: Pagination) -> ImpactListQuery {
        ImpactListQuery {
            limit: page.limit,
            offset: page.offset,
            include_deleted: self.include_deleted,
            repository: self.repo.clone(),
        }
    }
}

/// Arguments of the `get` verbs.
#[derive(Args, Debug)]
pub struct GetArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of the `history` verbs (KAIROS-T-0557).
#[derive(Args, Debug)]
pub struct HistoryArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// The most rows of each list, the versions and the activity (default
    /// 20, max 200)
    #[arg(long)]
    pub limit: Option<i64>,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of the `edit` verbs — the KAIROS-A-0004 optimistic-concurrency
/// content edit. The CLI fetches the current entity and bases the PATCH on
/// its version unless `--version` overrides it; a stale version gets the
/// 409 rendering with the server-current guidance.
#[derive(Args, Debug)]
pub struct EditArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// New title (omit to keep the current title)
    #[arg(long)]
    pub title: Option<String>,
    /// New markdown content, full replacement (omit to keep)
    #[arg(long, conflicts_with = "content_file")]
    pub content: Option<String>,
    /// Read the new markdown content from a file (`-` reads stdin)
    #[arg(long, value_name = "FILE")]
    pub content_file: Option<PathBuf>,
    /// Base the edit on this version instead of the fetched current one
    /// (a stale value is rejected with 409 CONFLICT)
    #[arg(long)]
    pub version: Option<i32>,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `note` (KAIROS-T-0367): append a note at the end of the
/// item's content, with a heading that has the date and your name.
#[derive(Args, Debug)]
pub struct NoteArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// The markdown text of the note
    #[arg(
        long,
        conflicts_with = "text_file",
        required_unless_present = "text_file"
    )]
    pub text: Option<String>,
    /// Read the markdown text of the note from a file (`-` reads stdin)
    #[arg(long, value_name = "FILE")]
    pub text_file: Option<PathBuf>,
    #[command(flatten)]
    pub common: Common,
}

impl NoteArgs {
    /// The text of the note: `--text`, or the file of `--text-file`.
    pub fn text(&self) -> Result<String, CliError> {
        Ok(read_text(
            self.text.as_deref(),
            self.text_file.as_deref(),
            "--text-file",
        )?
        .unwrap_or_default())
    }
}

/// Arguments of `promote-note` (KAIROS-T-0505): turn a note into a draft
/// ADR or document.
#[derive(Args, Debug)]
pub struct PromoteNoteArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// The id of the note, as `get` lists it (e.g. n-3)
    pub note_id: String,
    /// What the note becomes: adr or document
    #[arg(long, value_parser = ["adr", "document"])]
    pub kind: String,
    /// The board that gets the new item (slug or UUID): an ADR board for
    /// an ADR, the owner board for a document
    #[arg(long, value_name = "BOARD")]
    pub board: String,
    /// The title of the new item (default: the first line of the note)
    #[arg(long)]
    pub title: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl EditArgs {
    /// The PATCH body: flags merged over the fetched current entity.
    pub fn build_request(
        &self,
        current_version: i32,
        current_content: &str,
    ) -> Result<UpdateContentRequest, CliError> {
        let content = read_text(
            self.content.as_deref(),
            self.content_file.as_deref(),
            "--content-file",
        )?;
        if self.title.is_none() && content.is_none() {
            return Err(CliError::Failure(
                "The command has no change. Use --title, --content or --content-file.".to_string(),
            ));
        }
        Ok(UpdateContentRequest {
            title: self.title.clone(),
            content: content.unwrap_or_else(|| current_content.to_string()),
            version: self.version.unwrap_or(current_version),
        })
    }
}

/// Arguments of the `transition` verbs.
#[derive(Args, Debug)]
pub struct TransitionArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// Target column: a name of a column of the board of the item, or an
    /// id (UUID). A move outside the board's transition graph is rejected
    /// with 422 listing the allowed targets
    #[arg(long = "to", value_name = "COLUMN")]
    pub to_column: String,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `kairos tasks move` (KAIROS-I-0012).
#[derive(Args, Debug)]
pub struct MoveArgs {
    /// The task's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// Target delivery board (slug or UUID). The task lands in that
    /// board's entry column and follows its team; you need `manage_tasks`
    /// on both boards. The task keeps its repository.
    #[arg(long = "to-board", value_name = "BOARD")]
    pub to_board: String,
    /// Also give the task the next code of the target board. The old code
    /// is retired, and each reference to it in the text of the items
    /// changes to the new code. A code in a URL or a path does not change.
    /// Without this flag, the task keeps its code
    #[arg(long)]
    pub rename: bool,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `kairos tasks hand-off` (KAIROS-T-0359).
#[derive(Args, Debug)]
pub struct HandOffArgs {
    /// The task's short code (e.g. ACME-T-0001). The task must be in Active
    pub short_code: String,
    /// The person who gets the claim: a user id, an email or a user name
    /// of a person of the organization, or `me`. The person must hold
    /// `transition_items` on the board of the task: by its team, by a
    /// grant, or as an organization admin
    #[arg(long = "to", value_name = "PERSON")]
    pub to: String,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `kairos documents move` (COLLIERY-T-0269).
#[derive(Args, Debug)]
pub struct DocumentMoveArgs {
    /// The document's short code (e.g. ACME-D-0001)
    pub short_code: String,
    /// The new owner board (slug or UUID), of each level. The owner board
    /// gives the right to edit the document. You need `manage_documents`
    /// on the board that owns the document now and on this board. Each
    /// document has an owner board, so you cannot remove it
    #[arg(long = "to-board", value_name = "BOARD")]
    pub to_board: String,
    /// Also give the document the next code of its new owner board. The
    /// old code is retired, and each reference to it in the text of the
    /// items changes to the new code
    #[arg(long)]
    pub rename: bool,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of the `archive` verbs (a soft delete, KAIROS-A-0001
/// cascade; the old name `delete` stays as an alias).
#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// Archive the item (the archive cascades to its children; a restore
    /// brings each one back)
    #[arg(long)]
    pub confirm: bool,
    /// Archive nothing: show the items that the archive takes, and the
    /// items that it cannot reach. Not with --confirm
    #[arg(long, conflicts_with = "confirm")]
    pub preview: bool,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `kairos tasks cancel` (KAIROS-T-0362).
#[derive(Args, Debug)]
pub struct CancelArgs {
    /// The task's short code (e.g. ACME-T-0001). The task must not be done
    pub short_code: String,
    /// Why the task is cancelled. Must not be empty
    #[arg(long, value_name = "TEXT")]
    pub reason: String,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `kairos tasks lane` (KAIROS-T-0433).
#[derive(Args, Debug)]
pub struct LaneArgs {
    /// The task's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// The lane: planned or support
    #[arg(value_name = "LANE", value_parser = ["planned", "support"])]
    pub work_class: String,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `kairos tasks review` (KAIROS-T-0718).
#[derive(Args, Debug)]
pub struct ReviewArgs {
    /// The task's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// The review state: not_needed, needed or done
    #[arg(value_name = "REVIEW", value_parser = ["not_needed", "needed", "done"])]
    pub review: String,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `kairos initiatives complexity`.
#[derive(Args, Debug)]
pub struct ComplexityArgs {
    /// The initiative's short code (e.g. ACME-I-0001)
    pub short_code: String,
    /// The complexity: xs, s, m, l or xl; none clears it
    #[arg(value_name = "COMPLEXITY", value_parser = ["xs", "s", "m", "l", "xl", "none"])]
    pub complexity: String,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `kairos initiatives|tasks set-parent` (KAIROS-T-0572).
/// Give the new parent, or `--remove`.
#[derive(Args, Debug)]
#[command(group(
    clap::ArgGroup::new("new_parent")
        .required(true)
        .args(["parent", "remove"])
))]
pub struct SetParentArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// The short code of the new parent: a strategy for an initiative, an
    /// initiative for a task. It replaces the old parent
    #[arg(value_name = "PARENT")]
    pub parent: Option<String>,
    /// Remove the parent
    #[arg(long)]
    pub remove: bool,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `kairos documents lifecycle` (KAIROS-T-0555).
#[derive(Args, Debug)]
pub struct LifecycleArgs {
    /// The document's short code (e.g. ACME-D-0001)
    pub short_code: String,
    /// The lifecycle of the document after the call. Each move is possible
    #[arg(
        long = "to",
        value_name = "STATE",
        value_parser = ["draft", "review", "published", "archived"]
    )]
    pub to: String,
    #[command(flatten)]
    pub common: Common,
}

/// `kairos <noun> metadata` (KAIROS-T-0591): write the metadata values of
/// an item. The fields are the metadata definitions of the organization
/// (`kairos metadata list`).
#[derive(clap::Subcommand, Debug)]
pub enum ItemMetadataCommand {
    /// Set metadata values of an item: each value is SLUG=VALUE. The server
    /// checks each value against its definition (an enum option, a date
    /// YYYY-MM-DD). The edit rule applies
    Set(MetadataSetArgs),
    /// Remove metadata values of an item, by the slug of the field. The
    /// edit rule applies
    Unset(MetadataUnsetArgs),
}

/// Arguments of `kairos <noun> metadata set`.
#[derive(Args, Debug)]
pub struct MetadataSetArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// One or more values, each `SLUG=VALUE` (e.g. priority=high)
    #[arg(
        value_name = "SLUG=VALUE",
        required = true,
        num_args = 1..,
        value_parser = parse_metadata_pair
    )]
    pub values: Vec<(String, String)>,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of `kairos <noun> metadata unset`.
#[derive(Args, Debug)]
pub struct MetadataUnsetArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// One or more slugs of fields (e.g. priority)
    #[arg(value_name = "SLUG", required = true, num_args = 1..)]
    pub slugs: Vec<String>,
    #[command(flatten)]
    pub common: Common,
}

/// The parser of one `SLUG=VALUE` of `metadata set`. A pair without `=`,
/// an empty slug or an empty value is a usage error (exit 64).
fn parse_metadata_pair(text: &str) -> Result<(String, String), String> {
    let Some((slug, value)) = text.split_once('=') else {
        return Err(format!(
            "{text:?} has no `=`. Give the slug of the field, `=` and the value, for example \
             priority=high."
        ));
    };
    if slug.is_empty() {
        return Err(format!("{text:?} has no slug before `=`."));
    }
    if value.is_empty() {
        return Err(format!(
            "{slug}= has no value. To remove the value, use `metadata unset`."
        ));
    }
    Ok((slug.to_string(), value.to_string()))
}

/// The body of `metadata set` or `metadata unset`: slug to value, `None`
/// to remove. A slug that the command gives twice is refused.
fn metadata_request<'a>(
    entries: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> Result<UpdateMetadataRequest, CliError> {
    let mut values = std::collections::BTreeMap::new();
    for (slug, value) in entries {
        if values
            .insert(slug.to_string(), value.map(str::to_string))
            .is_some()
        {
            return Err(CliError::Failure(format!(
                "The field {slug} is in the command more than once. Give each field once."
            )));
        }
    }
    Ok(UpdateMetadataRequest { values })
}

impl ItemMetadataCommand {
    pub async fn run(self, kind: EntityKind) -> Result<(), CliError> {
        let (short_code, common, request) = match &self {
            Self::Set(args) => (
                &args.short_code,
                &args.common,
                metadata_request(
                    args.values
                        .iter()
                        .map(|(slug, value)| (slug.as_str(), Some(value.as_str()))),
                )?,
            ),
            Self::Unset(args) => (
                &args.short_code,
                &args.common,
                metadata_request(args.slugs.iter().map(|slug| (slug.as_str(), None)))?,
            ),
        };
        let client = client(common)?;
        let response = client.update_metadata(kind, short_code, &request).await?;
        emit_metadata(common, &response)
    }
}

/// `metadata set` and `metadata unset` output: the values of the item
/// after the call.
pub fn emit_metadata(common: &Common, response: &ItemMetadataResponse) -> Result<(), CliError> {
    if common.json {
        return print_json(response);
    }
    println!("Kairos changed the metadata of {}.", response.short_code);
    for line in metadata_lines(&response.values) {
        println!("{line}");
    }
    Ok(())
}

/// Arguments of `kairos tasks purge` (KAIROS-T-0362).
#[derive(Args, Debug)]
pub struct PurgeArgs {
    /// The current short code of an archived task (e.g. ACME-T-0001)
    pub short_code: String,
    /// Delete the task for good. You cannot undo it
    #[arg(long)]
    pub confirm: bool,
    #[command(flatten)]
    pub common: Common,
}

/// The `--confirm` guard of `archive`: the text says that a restore
/// brings the item back.
pub fn require_archive_confirm(confirm: bool, what: &str) -> Result<(), CliError> {
    if confirm {
        Ok(())
    } else {
        Err(CliError::Failure(format!(
            "The archive of {what} needs --confirm. Run the command again with --confirm. \
             A restore brings the item back."
        )))
    }
}

/// The `--confirm` guard of `tasks purge`: the text says that nothing can
/// bring the task back.
pub fn require_purge_confirm(confirm: bool, what: &str) -> Result<(), CliError> {
    if confirm {
        Ok(())
    } else {
        Err(CliError::Failure(format!(
            "The purge deletes {what} for good, and you cannot undo it. Archive the task \
             first with `kairos tasks archive`. Then run the command again with --confirm."
        )))
    }
}

/// `tasks reopen` output (KAIROS-T-0564).
pub fn emit_reopened(common: &Common, task: &Task) -> Result<(), CliError> {
    if common.json {
        return print_json(task);
    }
    println!(
        "Kairos reopened {}. It is in the first column of its board that is not done.",
        task.short_code
    );
    Ok(())
}

/// `tasks cancel` output (KAIROS-T-0362).
pub fn emit_cancelled(common: &Common, task: &Task) -> Result<(), CliError> {
    if common.json {
        return print_json(task);
    }
    match &task.cancellation {
        Some(mark) => println!(
            "Kairos cancelled {}. It is in the done column now.\nReason: {}",
            task.short_code, mark.reason
        ),
        None => println!("Kairos cancelled {}.", task.short_code),
    }
    Ok(())
}

/// `tasks lane` output (KAIROS-T-0433). `before` is the lane before the
/// call: a call that changes nothing is a normal 200.
pub fn emit_lane(common: &Common, task: &Task) -> Result<(), CliError> {
    if common.json {
        return print_json(task);
    }
    println!("{}", lane_line(&task.short_code, &task.work_class));
    Ok(())
}

/// `tasks review` output (KAIROS-T-0718): the review state of the task.
pub fn emit_review(common: &Common, task: &Task) -> Result<(), CliError> {
    if common.json {
        return print_json(task);
    }
    println!("{}", review_line(task));
    Ok(())
}

/// The line of [`emit_review`].
fn review_line(task: &Task) -> String {
    match &task.review {
        Some(review) => format!(
            "Review of {}: {} (set by {}).",
            task.short_code, review.review, review.updated_by_name
        ),
        None => format!("{} has no review state.", task.short_code),
    }
}

/// The line of [`emit_lane`]: the lane of the task after the call, from
/// the answer of the server (KAIROS-T-0607). The CLI does not read the task
/// first, so it does not say whether the lane changed.
fn lane_line(short_code: &str, work_class: &str) -> String {
    format!("{short_code} is in the {work_class} lane.")
}

/// The rows of each list of `<noun> history` when `--limit` is not given:
/// the default of MCP `get_history`.
const HISTORY_LIMIT: i64 = 20;

/// `<noun> history` output (KAIROS-T-0557): the lines of MCP `get_history`.
/// `--json` prints the two lists in the list shape of the CLI
/// (KAIROS-T-0644): `items`, `total` and `next_offset`, under `versions`
/// and `activity`.
pub fn emit_history(
    common: &Common,
    short_code: &str,
    versions: &ListEnvelope<HistoryVersion>,
    activity: &ListEnvelope<ItemActivityEntry>,
) -> Result<(), CliError> {
    if common.json {
        return print_json(&serde_json::json!({
            "versions": list_json(versions),
            "activity": list_json(activity),
        }));
    }
    print!("{}", history_text(short_code, versions, activity));
    Ok(())
}

/// One server page in the list shape of the CLI (KAIROS-T-0644).
fn list_json<T: utoipa::ToSchema>(page: &ListEnvelope<T>) -> crate::listing::ListJson<'_, T> {
    crate::listing::ListJson {
        items: &page.items,
        total: page.total,
        next_offset: crate::listing::next_offset(page.total, page.offset, page.items.len()),
    }
}

/// The text of [`emit_history`]. Pure.
fn history_text(
    short_code: &str,
    versions: &ListEnvelope<HistoryVersion>,
    activity: &ListEnvelope<ItemActivityEntry>,
) -> String {
    let who = |name: Option<&str>, id: &str, agent: bool| {
        let name = name.unwrap_or(id);
        if agent {
            format!("{name} (agent)")
        } else {
            name.to_string()
        }
    };
    let mut out = format!("# History of {short_code}\n");
    for version in &versions.items {
        out.push_str(&format!(
            "- v{} — {} by {}\n",
            version.version,
            seconds(&version.edited_at),
            who(
                version.edited_by_name.as_deref(),
                &version.edited_by,
                version.agent_key_id.is_some()
            )
        ));
    }
    out.push_str(&older_line(versions.total, versions.items.len()));
    out.push_str("\n## Activity\n");
    if activity.items.is_empty() {
        out.push_str("No activity is recorded for this item.\n");
        return out;
    }
    for row in &activity.items {
        out.push_str(&format!(
            "- {} — {} by {}\n",
            row.text,
            seconds(&row.occurred_at),
            who(
                row.actor_name.as_deref(),
                &row.actor_id,
                row.agent_key_id.is_some()
            )
        ));
    }
    out.push_str(&older_line(activity.total, activity.items.len()));
    out
}

/// The line under a list that does not show each row, or nothing.
fn older_line(total: i64, shown: usize) -> String {
    let older = total.saturating_sub(i64::try_from(shown).unwrap_or(i64::MAX));
    match older {
        ..=0 => String::new(),
        1 => "- 1 older row is not shown. Use a larger --limit to see it.\n".to_string(),
        _ => format!("- {older} older rows are not shown. Use a larger --limit to see them.\n"),
    }
}

/// An RFC 3339 time to the second, as MCP `get_history` writes it. A text
/// that is not a time stays as it is.
fn seconds(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339).map_or_else(
        |_| rfc3339.to_string(),
        |time| {
            time.with_timezone(&chrono::Utc)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        },
    )
}

/// `set-parent` output (KAIROS-T-0572).
pub fn emit_parent(common: &Common, response: &SetParentResponse) -> Result<(), CliError> {
    if common.json {
        return print_json(response);
    }
    println!("{}", parent_line(response));
    Ok(())
}

/// The line of [`emit_parent`].
fn parent_line(response: &SetParentResponse) -> String {
    let was = response.previous_parent.as_deref().unwrap_or("none");
    match (&response.parent, response.changed) {
        (Some(parent), true) => format!(
            "The parent of {} is {parent} (was {was}).",
            response.short_code
        ),
        (None, true) => format!("{} has no parent now (was {was}).", response.short_code),
        (Some(parent), false) => format!(
            "{} has the parent {parent} already. Nothing changed.",
            response.short_code
        ),
        (None, false) => format!("{} has no parent. Nothing changed.", response.short_code),
    }
}

/// `tasks purge` output (KAIROS-T-0362).
pub fn emit_purged(
    common: &Common,
    response: &kairos_client::types::PurgeTaskResponse,
) -> Result<(), CliError> {
    if common.json {
        return print_json(response);
    }
    println!(
        "Kairos deleted {} (\"{}\") for good. No restore can bring it back.",
        response.short_code, response.title
    );
    Ok(())
}

/// The client-side `--confirm` guard for destructive verbs.
pub fn require_confirm(confirm: bool, what: &str) -> Result<(), CliError> {
    if confirm {
        Ok(())
    } else {
        Err(CliError::Failure(format!(
            "The delete of {what} needs --confirm. Run the command again with --confirm."
        )))
    }
}

// ---------------------------------------------------------------------------
// Rendering: one trait over the five entity DTOs
// ---------------------------------------------------------------------------

/// The rendering surface the five entity DTOs share: identity, versioning,
/// content, table columns, and the detail fields of `get`. The `ToSchema`
/// supertrait exists only because `ListEnvelope<T>` requires it.
pub trait EntityView: serde::Serialize + utoipa::ToSchema {
    /// Human noun ("task", "initiative", ...).
    const NOUN: &'static str;
    /// Column headers of the `list` table.
    const HEADERS: &'static [&'static str];
    fn short_code(&self) -> &str;
    fn title(&self) -> &str;
    fn version(&self) -> i32;
    fn content(&self) -> &str;
    fn column_id(&self) -> Option<&str>;
    /// The board of the item (an off-board ADR has none).
    fn board_id(&self) -> Option<&str>;
    /// When this work was put away, if it was (KAIROS-A-0020). `get`
    /// prints a banner for it: archived work reads normally, so without
    /// one there is nothing to tell a reader they are looking at a record
    /// rather than at live work.
    fn archived_at(&self) -> Option<&str>;
    /// One `list` table row, matching [`Self::HEADERS`].
    fn table_row(&self) -> Vec<String>;
    /// `(label, value)` detail lines for `get` (content rendered
    /// separately).
    fn fields(&self) -> Vec<(&'static str, String)>;
    /// The notes of the item (KAIROS-T-0630): the read of one item has
    /// them; a list does not.
    fn notes(&self) -> &[ItemNote];
}

fn or_dash(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "-".to_string())
}

/// The repositories of the `impacts` links of an item, by slug
/// (COLLIERY-T-0269), or `-`. An archived repository is marked.
fn impacts_cell(impacts: &[Impact]) -> String {
    if impacts.is_empty() {
        return "-".to_string();
    }
    impacts
        .iter()
        .map(|impact| match impact.repository.archived_at {
            Some(_) => format!("{} [archived]", impact.repository.slug),
            None => impact.repository.slug.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl EntityView for Strategy {
    const NOUN: &'static str = "strategy";
    const HEADERS: &'static [&'static str] = &["CODE", "TITLE", "VER", "UPDATED"];

    fn short_code(&self) -> &str {
        &self.short_code
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn version(&self) -> i32 {
        self.version
    }
    fn content(&self) -> &str {
        &self.content
    }
    fn notes(&self) -> &[ItemNote] {
        self.notes.as_deref().unwrap_or_default()
    }
    fn column_id(&self) -> Option<&str> {
        Some(&self.column_id)
    }
    fn board_id(&self) -> Option<&str> {
        Some(&self.board_id)
    }
    fn archived_at(&self) -> Option<&str> {
        self.archived_at.as_deref()
    }
    fn table_row(&self) -> Vec<String> {
        vec![
            self.short_code.clone(),
            self.title.clone(),
            self.version.to_string(),
            self.updated_at.clone(),
        ]
    }
    fn fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("id", self.id.clone()),
            ("board", self.board_id.clone()),
            ("column", self.column_id.clone()),
            ("hypothesis", or_dash(&self.hypothesis)),
            ("version", self.version.to_string()),
            ("created", self.created_at.clone()),
            ("updated", self.updated_at.clone()),
        ]
    }
}

impl EntityView for Initiative {
    const NOUN: &'static str = "initiative";
    const HEADERS: &'static [&'static str] =
        &["CODE", "TITLE", "COMPLEXITY", "BUCKET", "VER", "UPDATED"];

    fn short_code(&self) -> &str {
        &self.short_code
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn version(&self) -> i32 {
        self.version
    }
    fn content(&self) -> &str {
        &self.content
    }
    fn notes(&self) -> &[ItemNote] {
        self.notes.as_deref().unwrap_or_default()
    }
    fn column_id(&self) -> Option<&str> {
        Some(&self.column_id)
    }
    fn board_id(&self) -> Option<&str> {
        Some(&self.board_id)
    }
    fn archived_at(&self) -> Option<&str> {
        self.archived_at.as_deref()
    }
    fn table_row(&self) -> Vec<String> {
        vec![
            self.short_code.clone(),
            self.title.clone(),
            or_dash(&self.complexity),
            or_dash(&self.bucket_type),
            self.version.to_string(),
            self.updated_at.clone(),
        ]
    }
    fn fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("id", self.id.clone()),
            ("board", self.board_id.clone()),
            ("column", self.column_id.clone()),
            ("complexity", or_dash(&self.complexity)),
            ("bucket", or_dash(&self.bucket_type)),
            ("version", self.version.to_string()),
            ("created", self.created_at.clone()),
            ("updated", self.updated_at.clone()),
        ]
    }
}

impl EntityView for Task {
    const NOUN: &'static str = "task";
    const HEADERS: &'static [&'static str] = &["CODE", "TITLE", "TYPE", "VER", "UPDATED"];

    fn short_code(&self) -> &str {
        &self.short_code
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn version(&self) -> i32 {
        self.version
    }
    fn content(&self) -> &str {
        &self.content
    }
    fn notes(&self) -> &[ItemNote] {
        self.notes.as_deref().unwrap_or_default()
    }
    fn column_id(&self) -> Option<&str> {
        Some(&self.column_id)
    }
    fn board_id(&self) -> Option<&str> {
        Some(&self.board_id)
    }
    fn archived_at(&self) -> Option<&str> {
        self.archived_at.as_deref()
    }
    fn table_row(&self) -> Vec<String> {
        vec![
            self.short_code.clone(),
            self.title.clone(),
            self.task_type.clone(),
            self.version.to_string(),
            self.updated_at.clone(),
        ]
    }
    fn fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("id", self.id.clone()),
            ("board", self.board_id.clone()),
            ("column", self.column_id.clone()),
            ("type", self.task_type.clone()),
            ("team", or_dash(&self.team_id)),
            // KAIROS-T-0591: the repository by its slug.
            (
                "repository",
                self.repository
                    .as_ref()
                    .map(|repository| repository.slug.clone())
                    .or_else(|| self.repository_id.clone())
                    .unwrap_or_else(|| "-".to_string()),
            ),
            (
                "claim",
                self.claim.as_ref().map_or_else(
                    || "-".to_string(),
                    |c| format!("{} since {}", claim_holder(c), c.claimed_at),
                ),
            ),
            (
                "cancelled",
                self.cancellation.as_ref().map_or_else(
                    || "-".to_string(),
                    |c| {
                        format!(
                            "{} by {}. Reason: {}",
                            c.cancelled_at, c.cancelled_by_name, c.reason
                        )
                    },
                ),
            ),
            // KAIROS-T-0506: the live state of the task.
            (
                "state",
                self.state
                    .as_ref()
                    .map_or_else(|| "-".to_string(), crate::commands::state::state_text),
            ),
            ("version", self.version.to_string()),
            ("created", self.created_at.clone()),
            ("updated", self.updated_at.clone()),
        ]
    }
}

impl EntityView for Document {
    const NOUN: &'static str = "document";
    const HEADERS: &'static [&'static str] = &["CODE", "TITLE", "LIFECYCLE", "VER", "UPDATED"];

    fn short_code(&self) -> &str {
        &self.short_code
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn version(&self) -> i32 {
        self.version
    }
    fn content(&self) -> &str {
        &self.content
    }
    fn notes(&self) -> &[ItemNote] {
        self.notes.as_deref().unwrap_or_default()
    }
    fn column_id(&self) -> Option<&str> {
        None
    }
    fn board_id(&self) -> Option<&str> {
        Some(&self.board_id)
    }
    fn archived_at(&self) -> Option<&str> {
        self.archived_at.as_deref()
    }
    fn table_row(&self) -> Vec<String> {
        vec![
            self.short_code.clone(),
            self.title.clone(),
            self.lifecycle.clone(),
            self.version.to_string(),
            self.updated_at.clone(),
        ]
    }
    fn fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("id", self.id.clone()),
            // COLLIERY-T-0269. The board that the document names. Each
            // document has one (COLLIERY-T-3109).
            ("owner board", self.board_id.clone()),
            ("impacts", impacts_cell(&self.impacts)),
            ("template", or_dash(&self.template_id)),
            ("lifecycle", self.lifecycle.clone()),
            ("version", self.version.to_string()),
            ("created", self.created_at.clone()),
            ("updated", self.updated_at.clone()),
        ]
    }
}

impl EntityView for Adr {
    const NOUN: &'static str = "ADR";
    const HEADERS: &'static [&'static str] = &["CODE", "TITLE", "DECIDED", "VER", "UPDATED"];

    fn short_code(&self) -> &str {
        &self.short_code
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn version(&self) -> i32 {
        self.version
    }
    fn content(&self) -> &str {
        &self.content
    }
    fn notes(&self) -> &[ItemNote] {
        self.notes.as_deref().unwrap_or_default()
    }
    fn column_id(&self) -> Option<&str> {
        self.column_id.as_deref()
    }
    fn board_id(&self) -> Option<&str> {
        self.board_id.as_deref()
    }
    fn archived_at(&self) -> Option<&str> {
        self.archived_at.as_deref()
    }
    fn table_row(&self) -> Vec<String> {
        vec![
            self.short_code.clone(),
            self.title.clone(),
            or_dash(&self.decision_date),
            self.version.to_string(),
            self.updated_at.clone(),
        ]
    }
    fn fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("id", self.id.clone()),
            ("board", or_dash(&self.board_id)),
            ("column", or_dash(&self.column_id)),
            ("impacts", impacts_cell(&self.impacts)),
            ("decision maker", or_dash(&self.decision_maker)),
            ("decision date", or_dash(&self.decision_date)),
            ("version", self.version.to_string()),
            ("created", self.created_at.clone()),
            ("updated", self.updated_at.clone()),
        ]
    }
}

// ---------------------------------------------------------------------------
// Emit helpers (human table / --json raw DTO)
// ---------------------------------------------------------------------------

pub fn emit_list<T: EntityView>(common: &Common, envelope: &Listing<T>) -> Result<(), CliError> {
    if common.json {
        return envelope.print_json();
    }
    if envelope.items.is_empty() {
        println!("(none)");
    } else {
        let mut table = Table::new(T::HEADERS);
        for item in &envelope.items {
            let mut row = item.table_row();
            // Marked in place rather than in a column of its own: a
            // default listing holds no archived rows, so its table is
            // exactly what it was before --include-deleted existed, and a
            // widened one still cannot be misread (KAIROS-A-0020 rule 2).
            if item.archived_at().is_some()
                && let Some(code) = row.first_mut()
            {
                code.push_str(" [archived]");
            }
            table.row(row);
        }
        print!("{}", table.render());
    }
    envelope.print_footer();
    Ok(())
}

/// `show` output: the fields, then `context` (the lines of the metadata,
/// the relationships and the external references, KAIROS-T-0591), then the
/// content.
pub fn emit_get<T: EntityView>(
    common: &Common,
    item: &T,
    context: &[String],
) -> Result<(), CliError> {
    if common.json {
        return print_json(item);
    }
    println!("{}: {}", item.short_code(), item.title());
    if let Some(archived_at) = item.archived_at() {
        println!("  ARCHIVED: {archived_at}. You can read this item. It is not on a board.");
    }
    for (label, value) in item.fields() {
        println!("  {label}: {value}");
    }
    for line in context {
        println!("{line}");
    }
    println!();
    if item.notes().is_empty() {
        println!("{}", item.content());
    } else {
        println!("{}{}", item.content().trim_end(), notes_text(item.notes()));
    }
    Ok(())
}

/// The notes after the content in `get` (KAIROS-T-0630): each one under
/// its `## Note n-3 (date, name)` heading, as the MCP `get_item` shows
/// them. Empty for no notes.
fn notes_text(notes: &[ItemNote]) -> String {
    kairos_markdown::notes::render_notes(notes.iter().map(|note| {
        kairos_markdown::notes::NoteParts {
            seq: kairos_markdown::notes::parse_note_id(&note.id).unwrap_or_default(),
            date: note.created_at.get(..10).unwrap_or(&note.created_at),
            author: &note.author,
            text: &note.text,
            promoted_to: note.promoted_to.as_deref(),
        }
    }))
}

/// The lines of `show` for the metadata, the relationships and the
/// external references of an item (KAIROS-T-0591). Three reads, at the
/// same time. Each read includes an archived item.
async fn item_context(
    client: &KairosClient,
    kind: EntityKind,
    short_code: &str,
) -> Result<Vec<String>, CliError> {
    let (metadata, relationships, external) = tokio::try_join!(
        client.metadata(kind, short_code),
        client.relationships(kind, short_code),
        client.item_external_refs(kind, short_code),
    )?;
    let mut lines = metadata_lines(&metadata.values);
    lines.extend(relationship_lines(&relationships));
    lines.extend(external_lines(&external.external));
    Ok(lines)
}

/// A labelled list: `  label: -` when it is empty, else the label and one
/// indented line for each entry.
fn labelled(label: &str, entries: Vec<String>) -> Vec<String> {
    if entries.is_empty() {
        return vec![format!("  {label}: -")];
    }
    let mut lines = vec![format!("  {label}:")];
    lines.extend(entries.into_iter().map(|entry| format!("    {entry}")));
    lines
}

/// The lines of the metadata values: `slug: value`, by slug.
fn metadata_lines(values: &[MetadataValue]) -> Vec<String> {
    labelled(
        "metadata",
        values
            .iter()
            .map(|value| format!("{}: {}", value.slug, value.value))
            .collect(),
    )
}

/// The label of a relationship from the side of the item, and its rank
/// in the list. `outgoing` is true when the item is the source.
fn relationship_label(relationship: &str, outgoing: bool) -> (usize, String) {
    let known = match (relationship, outgoing) {
        ("parent", false) => Some((0, "parent")),
        ("parent", true) => Some((1, "child")),
        ("blocks", false) => Some((2, "blocked by")),
        ("blocks", true) => Some((3, "blocks")),
        ("supports", true) => Some((4, "supported by")),
        ("supports", false) => Some((5, "supports")),
        ("informs", false) => Some((6, "informed by")),
        ("informs", true) => Some((7, "informs")),
        ("supersedes", false) => Some((8, "superseded by")),
        ("supersedes", true) => Some((9, "supersedes")),
        // KAIROS-A-0069: the amendments in the order of the edges.
        ("amends", false) => Some((10, "amended by")),
        ("amends", true) => Some((11, "amends")),
        _ => None,
    };
    match known {
        Some((rank, label)) => (rank, label.to_string()),
        None if outgoing => (12, relationship.to_string()),
        None => (12, format!("{relationship} (from)")),
    }
}

/// One neighbour: `CODE — title`, with the marks `[archived]`,
/// `[cancelled]` or `[done]`.
fn neighbour_text(item: &RelatedItem) -> String {
    let mut text = format!("{} — {}", item.short_code, item.title);
    if item.archived_at.is_some() {
        text.push_str(" [archived]");
    }
    if item.cancelled {
        text.push_str(" [cancelled]");
    } else if item.done {
        text.push_str(" [done]");
    }
    text
}

/// The lines of the relationships of an item, both directions: parent,
/// children, blockers, then the documents and the ADRs.
fn relationship_lines(relationships: &ItemRelationshipsResponse) -> Vec<String> {
    let mut entries: Vec<(usize, String)> = Vec::new();
    for (groups, outgoing) in [
        (&relationships.incoming, false),
        (&relationships.outgoing, true),
    ] {
        for group in groups {
            let (rank, label) = relationship_label(&group.relationship, outgoing);
            for item in &group.items {
                entries.push((rank, format!("{label}: {}", neighbour_text(item))));
            }
        }
    }
    entries.sort_by_key(|(rank, _)| *rank);
    labelled(
        "relationships",
        entries.into_iter().map(|(_, entry)| entry).collect(),
    )
}

/// The lines of the external references of an item.
fn external_lines(references: &[kairos_client::types_meta::ExternalRef]) -> Vec<String> {
    labelled("external", references.iter().map(reference_text).collect())
}

pub fn emit_created<T: EntityView>(common: &Common, item: &T) -> Result<(), CliError> {
    if common.json {
        return print_json(item);
    }
    println!(
        "Kairos made the {} {} (version {}): {}",
        T::NOUN,
        item.short_code(),
        item.version(),
        item.title()
    );
    Ok(())
}

pub fn emit_edited<T: EntityView>(common: &Common, item: &T) -> Result<(), CliError> {
    if common.json {
        return print_json(item);
    }
    println!(
        "Kairos changed the {} {}. The new version is {}.",
        T::NOUN,
        item.short_code(),
        item.version()
    );
    Ok(())
}

/// `transition` output: the name of the column after the move
/// (KAIROS-T-0589), from the columns of the board of the item.
pub fn emit_transitioned<T: EntityView>(
    common: &Common,
    item: &T,
    detail: &kairos_client::types_org::BoardDetail,
) -> Result<(), CliError> {
    if common.json {
        return print_json(item);
    }
    println!(
        "{}",
        transitioned_line(T::NOUN, item.short_code(), item.column_id(), detail)
    );
    Ok(())
}

/// The line of [`emit_transitioned`]: the column by its name, or by its id
/// when the board has no such column. Pure.
fn transitioned_line(
    noun: &str,
    short_code: &str,
    column_id: Option<&str>,
    detail: &kairos_client::types_org::BoardDetail,
) -> String {
    let column = column_id.map_or_else(
        || "-".to_string(),
        |id| {
            detail
                .columns
                .iter()
                .find(|c| c.id == id)
                .map_or_else(|| id.to_string(), |c| c.name.clone())
        },
    );
    format!("Kairos moved the {noun} {short_code} to the column {column}.")
}

/// Render the claim of a task after a claim, a hand-off or a release
/// (KAIROS-T-0359, KAIROS-T-0528).
pub fn emit_claim(common: &Common, task: &Task) -> Result<(), CliError> {
    if common.json {
        return print_json(task);
    }
    println!("{}", claim_line(task));
    Ok(())
}

/// The sentence about the claim of a task.
fn claim_line(task: &Task) -> String {
    match &task.claim {
        Some(claim) => format!(
            "{} has the claim on the task {} since {}.",
            claim_holder(claim),
            task.short_code,
            claim.claimed_at
        ),
        None => format!(
            "The task {} has no claim. `kairos tasks claim` takes it; a move to Active gives it.",
            task.short_code
        ),
    }
}

/// "Alice (agent)" when the agent of the person made the claim.
fn claim_holder(claim: &kairos_client::types::TaskClaim) -> String {
    if claim.agent {
        format!("{} (agent)", claim.display_name)
    } else {
        claim.display_name.clone()
    }
}

/// Render a board move (KAIROS-I-0012). Tasks only — they are the one
/// family on per-team boards — so this takes the DTO directly rather than
/// widening [`EntityView`] with a board accessor.
pub fn emit_moved(common: &Common, before: &str, task: &Task) -> Result<(), CliError> {
    if common.json {
        return print_json(task);
    }
    println!(
        "Kairos moved the task {} to the board {} (column {}).",
        task.short_code, task.board_id, task.column_id
    );
    if let Some(line) = renamed_line(before, &task.short_code) {
        println!("{line}");
    }
    Ok(())
}

/// The notice of `get` when the code that was asked for is not the code
/// of the item that the server gave: the code is retired, and the server
/// followed it to the item (COLLIERY-T-3100). `None` when the codes are the
/// same.
fn retired_code_notice(asked: &str, current: &str) -> Option<String> {
    (!asked.eq_ignore_ascii_case(current)).then(|| {
        format!(
            "The code {asked} is retired. The current code of this item is {current}. Use \
             {current} for a change to the item."
        )
    })
}

/// The line for a move that gave the item a new code (COLLIERY-T-3101).
/// `None` when the code did not change.
fn renamed_line(before: &str, after: &str) -> Option<String> {
    (before != after).then(|| {
        format!(
            "Kairos gave it the code {after}. The code {before} is retired: a read with it \
             finds the item."
        )
    })
}

/// Render the change of the owner board of a document (COLLIERY-T-0269).
/// `before` is the owner board that the document named before the call.
pub fn emit_document_moved(common: &Common, document: &Document) -> Result<(), CliError> {
    if common.json {
        return print_json(document);
    }
    println!(
        "{}",
        document_moved_line(&document.short_code, &document.board_id)
    );
    Ok(())
}

/// The line of [`emit_document_moved`]: the owner board of the document
/// after the call, from the answer of the server (KAIROS-T-0607). The CLI
/// does not read the document first, so it does not say whether the board
/// changed.
fn document_moved_line(short_code: &str, board_id: &str) -> String {
    format!("The owner board of the document {short_code} is {board_id}.")
}

/// `documents lifecycle` output (KAIROS-T-0555).
pub fn emit_lifecycle(common: &Common, document: &Document) -> Result<(), CliError> {
    if common.json {
        return print_json(document);
    }
    println!(
        "{}",
        lifecycle_line(&document.short_code, &document.lifecycle)
    );
    Ok(())
}

/// `initiatives complexity` output: the complexity after the call, from the
/// answer of the server, as [`emit_lane`]. A call that changes nothing is a
/// normal 200.
pub fn emit_complexity(common: &Common, initiative: &Initiative) -> Result<(), CliError> {
    if common.json {
        return print_json(initiative);
    }
    println!(
        "{}",
        complexity_line(&initiative.short_code, initiative.complexity.as_deref())
    );
    Ok(())
}

/// The line of [`emit_complexity`].
fn complexity_line(short_code: &str, complexity: Option<&str>) -> String {
    match complexity {
        Some(complexity) => format!("{short_code} has the complexity {complexity}."),
        None => format!("{short_code} has no complexity."),
    }
}

/// The line of [`emit_lifecycle`]: the lifecycle of the document after the
/// call, from the answer of the server. The CLI does not read the document
/// first, so it does not say whether the lifecycle changed.
fn lifecycle_line(short_code: &str, lifecycle: &str) -> String {
    format!("The document {short_code} is {lifecycle}.")
}

/// `promote-note` output (KAIROS-T-0505): the new item and its edge.
pub fn emit_promoted(common: &Common, response: &PromoteNoteResponse) -> Result<(), CliError> {
    if common.json {
        return print_json(response);
    }
    let edge = if response.supports {
        format!(
            " {} supports {}.",
            response.promoted_to, response.short_code
        )
    } else {
        String::new()
    };
    println!(
        "Kairos promoted note {} on {} to the draft {} {}.{edge}",
        response.note_id, response.short_code, response.kind, response.promoted_to
    );
    Ok(())
}

/// `note` output (KAIROS-T-0367): the id of the note. A note writes no
/// version (KAIROS-T-0630).
pub fn emit_noted(common: &Common, response: &AppendNoteResponse) -> Result<(), CliError> {
    if common.json {
        return print_json(response);
    }
    println!(
        "Kairos appended note {} to {}. The content stays at version {}.",
        response.note_id, response.short_code, response.version
    );
    Ok(())
}

/// `restore` output: what came back, and what deliberately did not.
pub fn emit_restored(common: &Common, response: &RestoreResponse) -> Result<(), CliError> {
    if common.json {
        return print_json(response);
    }
    println!("Kairos restored {}.", response.short_code);
    if response.still_archived_count > 0 {
        // Named, not silently omitted: a cascade delete archived these, and
        // a restore deliberately does not resurrect a subtree nobody asked
        // to revisit (KAIROS-T-0160).
        println!(
            "  The number of items below it that stay in the archive is {}: {}.",
            response.still_archived_count,
            response.still_archived_short_codes.join(", ")
        );
        println!("  To get one back, restore it by its short code.");
    }
    Ok(())
}

pub fn emit_deleted(common: &Common, response: &DeleteResponse) -> Result<(), CliError> {
    if common.json {
        return print_json(response);
    }
    if response.cascade_count == 0 {
        println!("Kairos archived {}.", response.short_code);
    } else {
        println!(
            "Kairos archived {}. The archive took {} descendant{}: {}.",
            response.short_code,
            response.cascade_count,
            if response.cascade_count == 1 { "" } else { "s" },
            response.cascaded_short_codes.join(", ")
        );
    }
    // COLLIERY-T-0234: the archive stops at a descendant that the caller
    // cannot edit. Each one that stays is named, with the reason.
    for line in not_reached_lines(&response.not_reached) {
        println!("{line}");
    }
    Ok(())
}

/// The lines of `delete` for the descendants that the archive did not
/// reach (COLLIERY-T-0234). No line when the archive reached each one, so
/// that output is as it was.
fn not_reached_lines(items: &[kairos_client::types::NotReached]) -> Vec<String> {
    if items.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "  The archive did not reach {} item{}. They stay live and keep their parent.",
        items.len(),
        if items.len() == 1 { "" } else { "s" }
    )];
    for item in items {
        lines.push(format!(
            "    - {}: {}",
            item.short_code,
            not_reached_reason(item)
        ));
    }
    lines
}

/// Why the archive does not reach one descendant (COLLIERY-T-0234). Pure.
pub fn not_reached_reason(item: &kairos_client::types::NotReached) -> String {
    match (&item.below, &item.required_capability, &item.board_id) {
        (Some(stop), _, _) => format!("It is below {stop}."),
        (None, Some(capability), Some(board)) => {
            format!("You need {capability} on the board {board}.")
        }
        (None, Some(capability), None) => {
            format!("It has no board for {capability}. Ask an organization admin.")
        }
        (None, None, _) => "You cannot edit it.".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Per-family create arguments
// ---------------------------------------------------------------------------

/// The markdown of `--content` or of `--content-file` (`-` reads standard
/// input), or `None` when neither is given (KAIROS-T-0589).
pub fn read_text(
    text: Option<&str>,
    file: Option<&Path>,
    flags: &str,
) -> Result<Option<String>, CliError> {
    match (text, file) {
        (Some(text), _) => Ok(Some(text.to_string())),
        (None, Some(path)) if path == Path::new("-") => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).map_err(|err| {
                CliError::Failure(format!(
                    "The CLI cannot read standard input for {flags}: {err}."
                ))
            })?;
            Ok(Some(text))
        }
        (None, Some(path)) => std::fs::read_to_string(path).map(Some).map_err(|err| {
            CliError::Failure(format!("The CLI cannot read {}: {err}.", path.display()))
        }),
        (None, None) => Ok(None),
    }
}

/// The id of the column `--column` of a create (KAIROS-T-0589): a name in
/// the board `board`, or an id. A name with no board is an error.
async fn create_column(
    client: &KairosClient,
    board: Option<&str>,
    column: Option<&str>,
) -> Result<Option<String>, CliError> {
    match (column, board) {
        (None, _) => Ok(None),
        (Some(column), _) if is_uuid(column) => Ok(Some(column.to_string())),
        (Some(column), Some(board)) => Ok(Some(column_id(client, board, column).await?)),
        (Some(column), None) => Err(CliError::Failure(format!(
            "The column {column:?} is a name, and the command names no board. Give --board, \
             or the id of the column."
        ))),
    }
}

/// Arguments of `kairos strategies create`.
#[derive(Args, Debug)]
pub struct StrategyCreateArgs {
    /// Board to create the strategy on (slug or UUID)
    #[arg(long, value_name = "BOARD")]
    pub board: String,
    /// Column to place it in: a name or an id (UUID). Default: the first
    /// column of the board
    #[arg(long, value_name = "COLUMN")]
    pub column: Option<String>,
    /// Title
    #[arg(long)]
    pub title: String,
    /// Markdown content (defaults to empty)
    #[arg(long, conflicts_with = "content_file")]
    pub content: Option<String>,
    /// Read the markdown content from a file (`-` reads stdin)
    #[arg(long, value_name = "FILE")]
    pub content_file: Option<PathBuf>,
    /// Strategy hypothesis
    #[arg(long)]
    pub hypothesis: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl StrategyCreateArgs {
    async fn request(&self, client: &KairosClient) -> Result<CreateStrategyRequest, CliError> {
        Ok(CreateStrategyRequest {
            board_id: self.board.clone(),
            column_id: create_column(client, Some(&self.board), self.column.as_deref()).await?,
            title: self.title.clone(),
            content: read_text(
                self.content.as_deref(),
                self.content_file.as_deref(),
                "--content-file",
            )?
            .unwrap_or_default(),
            hypothesis: self.hypothesis.clone(),
        })
    }
}

/// Arguments of `kairos initiatives create`.
#[derive(Args, Debug)]
pub struct InitiativeCreateArgs {
    /// Board to create the initiative on (slug or UUID)
    #[arg(long, value_name = "BOARD")]
    pub board: String,
    /// Column to place it in: a name or an id (UUID). Default: the first
    /// column of the board
    #[arg(long, value_name = "COLUMN")]
    pub column: Option<String>,
    /// Title
    #[arg(long)]
    pub title: String,
    /// Markdown content (defaults to empty)
    #[arg(long, conflicts_with = "content_file")]
    pub content: Option<String>,
    /// Read the markdown content from a file (`-` reads stdin)
    #[arg(long, value_name = "FILE")]
    pub content_file: Option<PathBuf>,
    /// T-shirt sizing: xs|s|m|l|xl
    #[arg(long)]
    pub complexity: Option<String>,
    /// Mark as a bucket of this kind: tech_debt|bug|ad_hoc
    #[arg(long, value_name = "KIND")]
    pub bucket_type: Option<String>,
    /// The parent strategy (short code). The initiative and the edge are
    /// one write: a refused parent leaves no initiative
    #[arg(long, value_name = "STRATEGY")]
    pub parent: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl InitiativeCreateArgs {
    async fn request(&self, client: &KairosClient) -> Result<CreateInitiativeRequest, CliError> {
        Ok(CreateInitiativeRequest {
            board_id: self.board.clone(),
            column_id: create_column(client, Some(&self.board), self.column.as_deref()).await?,
            title: self.title.clone(),
            content: read_text(
                self.content.as_deref(),
                self.content_file.as_deref(),
                "--content-file",
            )?
            .unwrap_or_default(),
            complexity: self.complexity.clone(),
            bucket_type: self.bucket_type.clone(),
            parent: self.parent.clone(),
        })
    }
}

/// Arguments of `kairos tasks create`.
#[derive(Args, Debug)]
pub struct TaskCreateArgs {
    /// Delivery board to create the task on (slug or UUID). Optional when
    /// --team is given: the task goes to the delivery board of that team.
    /// The board decides the team of the task (COLLIERY-A-0023)
    #[arg(long, value_name = "BOARD", required_unless_present = "team")]
    pub board: Option<String>,
    /// The entry column of the board: a name or an id (UUID), the default.
    /// A task starts there; any other column is refused: move the task after
    /// the create
    #[arg(long, value_name = "COLUMN")]
    pub column: Option<String>,
    /// Title
    #[arg(long)]
    pub title: String,
    /// Markdown content (defaults to empty)
    #[arg(long, conflicts_with = "content_file")]
    pub content: Option<String>,
    /// Read the markdown content from a file (`-` reads stdin)
    #[arg(long, value_name = "FILE")]
    pub content_file: Option<PathBuf>,
    /// The kind of the task: task|bug|tech_debt|support (defaults to
    /// task). The old name --type still works
    #[arg(long = "kind", alias = "type", value_name = "KIND")]
    pub task_type: Option<String>,
    /// Planned/Support lane: planned|support (KAIROS-T-0077; defaults to
    /// support for support-type tasks, else planned)
    #[arg(long = "work-class", value_name = "WORK_CLASS")]
    pub work_class: Option<String>,
    /// Team (slug or UUID). Without --board, the task goes to the delivery
    /// board of this team. With --board, it must be the team of that board
    #[arg(long, value_name = "TEAM")]
    pub team: Option<String>,
    /// Repository the task links to (slug or UUID). It can be any
    /// repository, of any team. It does not choose the board
    #[arg(long, value_name = "REPOSITORY")]
    pub repo: Option<String>,
    /// The team that sends the task (slug or UUID, KAIROS-T-0426). When it
    /// is not the team of the board, the task is a request: entry column,
    /// support lane, also for a manager of the board
    #[arg(long = "from-team", value_name = "TEAM")]
    pub from_team: Option<String>,
    /// The parent initiative (short code). The task and the edge are one
    /// write: a refused parent leaves no task
    #[arg(long, value_name = "INITIATIVE")]
    pub parent: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl TaskCreateArgs {
    async fn request(&self, client: &KairosClient) -> Result<CreateTaskRequest, CliError> {
        let team = match &self.team {
            Some(team) => Some(team_id(client, team).await?),
            None => None,
        };
        // A column name needs a board: --board, else the delivery board of
        // --team.
        let board = match (&self.board, &team, &self.column) {
            (Some(board), _, _) => Some(board.clone()),
            (None, Some(team), Some(column)) if !is_uuid(column) => {
                client.get_team(team).await?.delivery_board_id
            }
            _ => None,
        };
        Ok(CreateTaskRequest {
            board_id: self.board.clone(),
            repository: self.repo.clone(),
            column_id: create_column(client, board.as_deref(), self.column.as_deref()).await?,
            title: self.title.clone(),
            content: read_text(
                self.content.as_deref(),
                self.content_file.as_deref(),
                "--content-file",
            )?
            .unwrap_or_default(),
            task_type: self.task_type.clone(),
            work_class: self.work_class.clone(),
            team_id: team,
            from_team: self.from_team.clone(),
            parent: self.parent.clone(),
        })
    }
}

/// Arguments of `kairos documents create`.
#[derive(Args, Debug)]
pub struct DocumentCreateArgs {
    /// Title
    #[arg(long)]
    pub title: String,
    /// The owner board of the document (slug or UUID), of each level. It
    /// gives the right to edit the document, and you need
    /// `manage_documents` on it. The code of the document gets the prefix
    /// of this board. The document is not a card of the board
    #[arg(long, value_name = "BOARD")]
    pub board: String,
    /// Short code of the workflow item this document supports (optional).
    /// It does not give the document an owner board
    #[arg(long, value_name = "SHORT_CODE")]
    pub parent: Option<String>,
    /// Markdown content (omit with --template to stamp the template's
    /// content)
    #[arg(long, conflicts_with = "content_file")]
    pub content: Option<String>,
    /// Read the markdown content from a file (`-` reads stdin)
    #[arg(long, value_name = "FILE")]
    pub content_file: Option<PathBuf>,
    /// Template to stamp content + metadata defaults from (slug or UUID)
    #[arg(long, value_name = "TEMPLATE")]
    pub template: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl DocumentCreateArgs {
    async fn request(&self, client: &KairosClient) -> Result<CreateDocumentRequest, CliError> {
        let template_id = match &self.template {
            Some(template) => Some(template_id(client, template).await?),
            None => None,
        };
        Ok(CreateDocumentRequest {
            title: self.title.clone(),
            board: self.board.clone(),
            content: read_text(
                self.content.as_deref(),
                self.content_file.as_deref(),
                "--content-file",
            )?,
            template_id,
            parent_short_code: self.parent.clone(),
        })
    }
}

/// Arguments of `kairos adrs create`.
#[derive(Args, Debug)]
pub struct AdrCreateArgs {
    /// Title
    #[arg(long)]
    pub title: String,
    /// ADR board (slug or UUID); omit for an off-board ADR (org-admin only)
    #[arg(long, value_name = "BOARD")]
    pub board: Option<String>,
    /// Column to place it in: a name (with --board) or an id (UUID).
    /// Default: the first column of the board
    #[arg(long, value_name = "COLUMN")]
    pub column: Option<String>,
    /// Markdown content (defaults to empty)
    #[arg(long, conflicts_with = "content_file")]
    pub content: Option<String>,
    /// Read the markdown content from a file (`-` reads stdin)
    #[arg(long, value_name = "FILE")]
    pub content_file: Option<PathBuf>,
    /// Decision maker
    #[arg(long)]
    pub decision_maker: Option<String>,
    /// Decision date (YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    pub decision_date: Option<String>,
    /// The item that the ADR supports (short code of a strategy, an
    /// initiative or a task). The ADR and the `supports` edge are one
    /// write: a refused parent leaves no ADR
    #[arg(long, value_name = "SHORT_CODE")]
    pub parent: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl AdrCreateArgs {
    async fn request(&self, client: &KairosClient) -> Result<CreateAdrRequest, CliError> {
        Ok(CreateAdrRequest {
            board_id: self.board.clone(),
            column_id: create_column(client, self.board.as_deref(), self.column.as_deref()).await?,
            title: self.title.clone(),
            content: read_text(
                self.content.as_deref(),
                self.content_file.as_deref(),
                "--content-file",
            )?
            .unwrap_or_default(),
            decision_maker: self.decision_maker.clone(),
            decision_date: self.decision_date.clone(),
            parent: self.parent.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// The family macro: one Subcommand enum + dispatcher per entity family
// ---------------------------------------------------------------------------

/// Generate the per-family `Subcommand` enum and its `run` dispatcher over
/// the corresponding `KairosClient` methods. The optional
/// `transition(Transition) = method` argument adds the transition verb
/// (documents have no transition endpoint in S-0005); the optional
/// `board_move(Move) = method` adds the board move (tasks only — they are
/// the one family on per-team boards, KAIROS-I-0012). The optional
/// `owner_move(Move) = method` adds the move of a document to a different
/// owner board (COLLIERY-T-0269). The optional `claims(Claim = method,
/// HandOff = method, Release = method)` adds the claim, the hand-off and
/// the release of the claim of a task in Active (tasks only,
/// KAIROS-T-0359, KAIROS-T-0528). `list_args` is the type of the flags of
/// `list`: [`ImpactListArgs`] for documents and ADRs, which have the
/// filter `--repo`.
macro_rules! entity_family_cli {
    (
        $enum_name:ident, $noun:literal, $create_args:ty, list_args = $list_args:ty,
        list = $list:ident, get = $get:ident, create = $create_fn:ident,
        update = $update:ident, delete = $delete:ident, restore = $restore:ident,
        kind = $kind:expr
        $(, transition($transition_variant:ident) = $transition_fn:ident)?
        $(, board_move($move_variant:ident) = $move_fn:ident)?
        $(, owner_move($owner_variant:ident) = $owner_fn:ident)?
        $(, claims($claim_variant:ident = $claim_fn:ident, $hand_variant:ident = $hand_fn:ident, $release_variant:ident = $release_fn:ident))?
        $(, task_end($cancel_variant:ident = $cancel_fn:ident, $reopen_variant:ident = $reopen_fn:ident, $purge_variant:ident = $purge_fn:ident))?
        $(, lane($lane_variant:ident = $lane_fn:ident))?
        $(, review($review_variant:ident = $review_fn:ident))?
        $(, parent($parent_variant:ident))?
        $(, lifecycle($lifecycle_variant:ident = $lifecycle_fn:ident))?
        $(, complexity($complexity_variant:ident = $complexity_fn:ident))?
    ) => {
        #[doc = concat!("Operations on ", $noun, "s.")]
        #[derive(clap::Subcommand, Debug)]
        pub enum $enum_name {
            // clap cannot evaluate `#[doc = concat!(..)]` (the tokens reach
            // the derive unexpanded), so per-verb help goes through
            // `command(about = ..)`, which clap emits as an expression.
            #[command(about = concat!("List ", $noun,
                                      "s (paginated; --include-deleted adds archived work)"))]
            List($list_args),
            #[command(about = concat!("Show one ", $noun,
                                      ", including its markdown content"),
                      alias = "get")]
            Show(GetArgs),
            #[command(about = concat!("Show the history of a ", $noun,
                                      ": its versions and its activity (moves, claims, ",
                                      "lane, metadata, state, links), newest first"))]
            History(HistoryArgs),
            #[command(about = concat!("Compare two versions of a ", $noun,
                                      ": the title and a unified diff of the content. ",
                                      "With one version, compare it with the version before"))]
            Diff(crate::commands::item_views::DiffArgs),
            #[command(about = concat!("List the branches, pull requests and issues ",
                                      "of the forge that link to a ", $noun))]
            Links(GetArgs),
            #[command(about = concat!("Create a ", $noun))]
            Create($create_args),
            #[command(about = concat!("Edit a ", $noun, "'s title and/or content ",
                                      "(version-checked; a concurrent edit gets 409)"))]
            Edit(EditArgs),
            #[command(about = concat!("Append a note at the end of a ", $noun,
                                      "'s content, under a heading with the date ",
                                      "and your name (no version needed)"))]
            Note(NoteArgs),
            #[command(about = concat!("Turn a note of a ", $noun,
                                      " into a draft ADR or document, linked to the ",
                                      $noun), name = "promote-note")]
            PromoteNote(PromoteNoteArgs),
            $(
            #[command(about = concat!("Move a ", $noun, " to another board column ",
                                      "(an invalid move lists the allowed targets)"))]
            $transition_variant(TransitionArgs),
            )?
            $(
            #[command(about = concat!("Move a ", $noun, " to another delivery board ",
                                      "(it lands in that board's entry column)"))]
            $move_variant(MoveArgs),
            )?
            $(
            #[command(about = concat!("Move a ", $noun, " to another owner board"))]
            $owner_variant(DocumentMoveArgs),
            )?
            $(
            #[command(about = concat!("Take the claim of a ", $noun,
                                      " in Active that has no claim"))]
            $claim_variant(GetArgs),
            #[command(about = concat!("Give the claim of a ", $noun,
                                      " in Active to a different person"))]
            $hand_variant(HandOffArgs),
            #[command(about = concat!("End the claim of a ", $noun,
                                      " in Active and leave it free for anyone"))]
            $release_variant(GetArgs),
            )?
            $(
            #[command(about = concat!("Cancel a ", $noun,
                                      " with a reason: it moves to the done column, ",
                                      "marked cancelled"))]
            $cancel_variant(CancelArgs),
            #[command(about = concat!("Reopen a done ", $noun,
                                      ": it moves to the first column that is not done, ",
                                      "and a cancel mark ends"))]
            $reopen_variant(GetArgs),
            #[command(about = concat!("Delete an archived ", $noun,
                                      " for good (requires --confirm; you cannot undo it; ",
                                      "needs the purge capability)"))]
            $purge_variant(PurgeArgs),
            )?
            $(
            #[command(about = concat!("Change the lane of a ", $noun,
                                      ": planned or support (needs transition_items)"))]
            $lane_variant(LaneArgs),
            )?
            $(
            #[command(about = concat!("Set the review state of a ", $noun,
                                      ": not_needed, needed or done (the edit rule applies)"))]
            $review_variant(ReviewArgs),
            )?
            $(
            #[command(about = concat!("Set the parent of a ", $noun,
                                      ", or remove it with --remove (the new parent ",
                                      "replaces the old one)"),
                      name = "set-parent")]
            $parent_variant(SetParentArgs),
            )?
            $(
            #[command(about = concat!("Set the lifecycle of a ", $noun,
                                      ": draft, review, published or archived ",
                                      "(the edit rule applies)"))]
            $lifecycle_variant(LifecycleArgs),
            )?
            $(
            #[command(about = concat!("Set the complexity of an ", $noun,
                                      ": xs, s, m, l or xl, or none to clear it ",
                                      "(the edit rule applies)"))]
            $complexity_variant(ComplexityArgs),
            )?
            #[command(about = concat!("Set or remove the metadata values of a ", $noun),
                      subcommand)]
            Metadata(ItemMetadataCommand),
            #[command(about = concat!("Archive a ", $noun,
                                      " and its children (requires --confirm; ",
                                      "`restore` brings it back; --preview archives nothing)"),
                      alias = "delete")]
            Archive(DeleteArgs),
            #[command(about = concat!("Put an archived ", $noun,
                                      " back on its board (its archived children stay archived)"))]
            Restore(GetArgs),
        }

        impl $enum_name {
            pub async fn run(self) -> Result<(), CliError> {
                match self {
                    Self::List(args) => {
                        let client = client(&args.common)?;
                        let listing = fetch(args.paging(), |page| {
                            client.$list(args.query(page))
                        })
                        .await?;
                        emit_list(&args.common, &listing)
                    }
                    Self::Show(args) => {
                        let client = client(&args.common)?;
                        let item = client.$get(&args.short_code).await?;
                        // COLLIERY-T-3100: a retired code finds the item.
                        // Say so on stderr, so that the output stays the
                        // item (and the JSON stays clean).
                        if let Some(notice) =
                            retired_code_notice(&args.short_code, &item.short_code)
                        {
                            eprintln!("{notice}");
                        }
                        // KAIROS-T-0591: the text gives the metadata, the
                        // relationships and the external references too.
                        // `--json` stays the item of the server.
                        let context = if args.common.json {
                            Vec::new()
                        } else {
                            item_context(&client, $kind, &item.short_code).await?
                        };
                        emit_get(&args.common, &item, &context)
                    }
                    Self::History(args) => {
                        let client = client(&args.common)?;
                        let limit = Some(args.limit.unwrap_or(HISTORY_LIMIT));
                        let versions =
                            client.history($kind, &args.short_code, limit, None).await?;
                        let activity =
                            client.item_activity($kind, &args.short_code, limit, None).await?;
                        emit_history(&args.common, &args.short_code, &versions, &activity)
                    }
                    Self::Diff(args) => {
                        let client = client(&args.common)?;
                        let (from, to) = args.versions()?;
                        let old = match from {
                            Some(version) => Some(
                                client
                                    .history_snapshot($kind, &args.short_code, version)
                                    .await?,
                            ),
                            None => None,
                        };
                        let new = client
                            .history_snapshot($kind, &args.short_code, to)
                            .await?;
                        crate::commands::item_views::emit_diff(
                            &args.common,
                            &args.short_code,
                            old.as_ref(),
                            &new,
                        )
                    }
                    Self::Links(args) => {
                        let client = client(&args.common)?;
                        let links = client.item_links($kind, &args.short_code).await?;
                        crate::commands::item_views::emit_links(
                            &args.common,
                            &args.short_code,
                            &links,
                        )
                    }
                    Self::Create(args) => {
                        let client = client(&args.common)?;
                        let request = args.request(&client).await?;
                        let item = client.$create_fn(&request).await?;
                        emit_created(&args.common, &item)
                    }
                    Self::Edit(args) => {
                        let client = client(&args.common)?;
                        let current = client.$get(&args.short_code).await?;
                        let request =
                            args.build_request(current.version, &current.content)?;
                        let item = client.$update(&args.short_code, &request).await?;
                        emit_edited(&args.common, &item)
                    }
                    Self::Note(args) => {
                        let client = client(&args.common)?;
                        let response =
                            client.append_note($kind, &args.short_code, &args.text()?).await?;
                        emit_noted(&args.common, &response)
                    }
                    Self::PromoteNote(args) => {
                        let client = client(&args.common)?;
                        let request = PromoteNoteRequest {
                            kind: args.kind.clone(),
                            board: args.board.clone(),
                            title: args.title.clone(),
                        };
                        let response = client
                            .promote_note($kind, &args.short_code, &args.note_id, &request)
                            .await?;
                        emit_promoted(&args.common, &response)
                    }
                    $(
                    Self::$transition_variant(args) => {
                        let client = client(&args.common)?;
                        // KAIROS-T-0589: the column is a name in the board
                        // of the item, or an id.
                        let current = client.$get(&args.short_code).await?;
                        let board = current.board_id().ok_or_else(|| {
                            CliError::Failure(format!(
                                "{} is on no board, so it has no column.",
                                current.short_code()
                            ))
                        })?;
                        let detail = client.get_board(board).await?;
                        let column = find_column(&detail, &args.to_column)?.id.clone();
                        let item = client.$transition_fn(&args.short_code, &column).await?;
                        emit_transitioned(&args.common, &item, &detail)
                    }
                    )?
                    $(
                    Self::$move_variant(args) => {
                        let client = client(&args.common)?;
                        let item = client
                            .$move_fn(&args.short_code, &args.to_board, args.rename)
                            .await?;
                        emit_moved(&args.common, &args.short_code, &item)
                    }
                    )?
                    $(
                    Self::$owner_variant(args) => {
                        let client = client(&args.common)?;
                        let item = client
                            .$owner_fn(&args.short_code, &args.to_board, args.rename)
                            .await?;
                        emit_document_moved(&args.common, &item)?;
                        if !args.common.json {
                            if let Some(line) = renamed_line(&args.short_code, &item.short_code) {
                                println!("{line}");
                            }
                        }
                        Ok(())
                    }
                    )?
                    $(
                    Self::$claim_variant(args) => {
                        let client = client(&args.common)?;
                        let task = client.$claim_fn(&args.short_code).await?;
                        emit_claim(&args.common, &task)
                    }
                    Self::$hand_variant(args) => {
                        let client = client(&args.common)?;
                        let task = client.$hand_fn(&args.short_code, &args.to).await?;
                        emit_claim(&args.common, &task)
                    }
                    Self::$release_variant(args) => {
                        let client = client(&args.common)?;
                        let task = client.$release_fn(&args.short_code).await?;
                        emit_claim(&args.common, &task)
                    }
                    )?
                    $(
                    Self::$cancel_variant(args) => {
                        let client = client(&args.common)?;
                        let task = client.$cancel_fn(&args.short_code, &args.reason).await?;
                        emit_cancelled(&args.common, &task)
                    }
                    Self::$reopen_variant(args) => {
                        let client = client(&args.common)?;
                        let task = client.$reopen_fn(&args.short_code).await?;
                        emit_reopened(&args.common, &task)
                    }
                    Self::$purge_variant(args) => {
                        require_purge_confirm(args.confirm, &args.short_code)?;
                        let client = client(&args.common)?;
                        let response = client.$purge_fn(&args.short_code).await?;
                        emit_purged(&args.common, &response)
                    }
                    )?
                    $(
                    Self::$lane_variant(args) => {
                        let client = client(&args.common)?;
                        let task = client.$lane_fn(&args.short_code, &args.work_class).await?;
                        emit_lane(&args.common, &task)
                    }
                    )?
                    $(
                    Self::$review_variant(args) => {
                        let client = client(&args.common)?;
                        let body = SetTaskReviewRequest {
                            review: args.review.clone(),
                        };
                        let task = client.$review_fn(&args.short_code, &body).await?;
                        emit_review(&args.common, &task)
                    }
                    )?
                    $(
                    Self::$parent_variant(args) => {
                        let client = client(&args.common)?;
                        let response = client
                            .set_parent($kind, &args.short_code, args.parent.as_deref())
                            .await?;
                        emit_parent(&args.common, &response)
                    }
                    )?
                    $(
                    Self::$lifecycle_variant(args) => {
                        let client = client(&args.common)?;
                        let document = client.$lifecycle_fn(&args.short_code, &args.to).await?;
                        emit_lifecycle(&args.common, &document)
                    }
                    )?
                    $(
                    Self::$complexity_variant(args) => {
                        let client = client(&args.common)?;
                        let initiative =
                            client.$complexity_fn(&args.short_code, &args.complexity).await?;
                        emit_complexity(&args.common, &initiative)
                    }
                    )?
                    Self::Metadata(command) => command.run($kind).await,
                    Self::Archive(args) if args.preview => {
                        let client = client(&args.common)?;
                        let preview = client.cascade_preview($kind, &args.short_code).await?;
                        crate::commands::item_views::emit_preview(&args.common, &preview)
                    }
                    Self::Archive(args) => {
                        require_archive_confirm(args.confirm, &args.short_code)?;
                        let client = client(&args.common)?;
                        let response = client.$delete(&args.short_code).await?;
                        emit_deleted(&args.common, &response)
                    }
                    Self::Restore(args) => {
                        let client = client(&args.common)?;
                        let response = client.$restore(&args.short_code).await?;
                        emit_restored(&args.common, &response)
                    }
                }
            }
        }
    };
}

entity_family_cli!(
    StrategiesCommand,
    "strategy",
    StrategyCreateArgs,
    list_args = EntityListArgs,
    list = list_strategies,
    get = get_strategy,
    create = create_strategy,
    update = update_strategy,
    delete = delete_strategy,
    restore = restore_strategy,
    kind = EntityKind::Strategy,
    transition(Transition) = transition_strategy
);

entity_family_cli!(
    InitiativesCommand,
    "initiative",
    InitiativeCreateArgs,
    list_args = EntityListArgs,
    list = list_initiatives,
    get = get_initiative,
    create = create_initiative,
    update = update_initiative,
    delete = delete_initiative,
    restore = restore_initiative,
    kind = EntityKind::Initiative,
    transition(Transition) = transition_initiative,
    parent(SetParent),
    complexity(Complexity = set_initiative_complexity)
);

entity_family_cli!(
    TasksCommand,
    "task",
    TaskCreateArgs,
    list_args = EntityListArgs,
    list = list_tasks,
    get = get_task,
    create = create_task,
    update = update_task,
    delete = delete_task,
    restore = restore_task,
    kind = EntityKind::Task,
    transition(Transition) = transition_task,
    board_move(Move) = move_task_with,
    claims(
        Claim = claim_task,
        HandOff = hand_off_task,
        Release = release_task
    ),
    task_end(
        Cancel = cancel_task,
        Reopen = reopen_task,
        Purge = purge_task
    ),
    lane(Lane = set_task_work_class),
    review(Review = set_task_review),
    parent(SetParent)
);

entity_family_cli!(
    DocumentsCommand,
    "document",
    DocumentCreateArgs,
    list_args = ImpactListArgs,
    list = list_documents,
    get = get_document,
    create = create_document,
    update = update_document,
    delete = delete_document,
    restore = restore_document,
    kind = EntityKind::Document,
    owner_move(Move) = set_document_board_with,
    lifecycle(Lifecycle = set_document_lifecycle)
);

entity_family_cli!(
    AdrsCommand,
    "ADR",
    AdrCreateArgs,
    list_args = ImpactListArgs,
    list = list_adrs,
    get = get_adr,
    create = create_adr,
    update = update_adr,
    delete = delete_adr,
    restore = restore_adr,
    kind = EntityKind::Adr,
    transition(Transition) = transition_adr
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EXIT_FAILURE;

    /// KAIROS-T-0630: `get` prints the notes after the content, each one
    /// under its heading, with the mark of a promote.
    #[test]
    fn the_notes_follow_the_content() {
        let notes = vec![
            ItemNote {
                id: "n-1".into(),
                author_id: None,
                author: "Robin".into(),
                created_at: "2026-10-17T09:30:00+00:00".into(),
                text: "One.\n\n## Findings\n\nTwo.".into(),
                promoted_to: Some("ACME-A-0001".into()),
            },
            ItemNote {
                id: "n-2".into(),
                author_id: None,
                author: "Sam".into(),
                created_at: "2026-10-18T10:00:00+00:00".into(),
                text: "Three.".into(),
                promoted_to: None,
            },
        ];
        assert_eq!(
            notes_text(&notes),
            "\n\n## Note n-1 (2026-10-17, Robin)\n\nOne.\n\n## Findings\n\nTwo.\n\n\
             Promoted to ACME-A-0001.\n\n## Note n-2 (2026-10-18, Sam)\n\nThree."
        );
        assert_eq!(notes_text(&[]), "");
    }

    /// COLLIERY-T-3100 / T-3101: `get` with a retired code says so, and a
    /// move that renames names the two codes. The same code says nothing.
    #[test]
    fn a_retired_code_and_a_rename_get_a_line() {
        assert_eq!(retired_code_notice("ACME-T-0001", "ACME-T-0001"), None);
        assert_eq!(retired_code_notice("acme-t-0001", "ACME-T-0001"), None);
        let notice = retired_code_notice("ACME-T-0001", "WEB-T-0004").expect("a notice");
        assert!(notice.contains("ACME-T-0001 is retired"), "{notice}");
        assert!(notice.contains("WEB-T-0004"), "{notice}");
        assert_eq!(renamed_line("ACME-T-0001", "ACME-T-0001"), None);
        let line = renamed_line("ACME-T-0001", "WEB-T-0004").expect("a line");
        assert!(line.contains("the code WEB-T-0004"), "{line}");
        assert!(line.contains("ACME-T-0001 is retired"), "{line}");
    }

    /// COLLIERY-T-0234: `delete` names each descendant that stays, with
    /// the reason, and prints no line when none stays.
    #[test]
    fn delete_names_what_the_archive_did_not_reach() {
        use kairos_client::types::NotReached;

        assert!(not_reached_lines(&[]).is_empty());
        let lines = not_reached_lines(&[
            NotReached {
                short_code: "ACME-I-0002".into(),
                required_capability: Some("manage_initiatives".into()),
                board_id: Some("b-1".into()),
                below: None,
            },
            NotReached {
                short_code: "ACME-T-0009".into(),
                required_capability: None,
                board_id: None,
                below: Some("ACME-I-0002".into()),
            },
        ]);
        assert_eq!(
            lines,
            vec![
                "  The archive did not reach 2 items. They stay live and keep their parent."
                    .to_string(),
                "    - ACME-I-0002: You need manage_initiatives on the board b-1.".to_string(),
                "    - ACME-T-0009: It is below ACME-I-0002.".to_string(),
            ]
        );
    }

    /// The edit flow's request building: flags merge over the fetched
    /// current entity; `--version` overrides the fetched version; no flags
    /// at all is a usage error.
    #[test]
    fn edit_request_building() {
        let base = EditArgs {
            short_code: "ACME-T-0001".into(),
            title: None,
            content: None,
            content_file: None,
            version: None,
            common: Common::default(),
        };

        // Nothing to change → usage failure (exit 1).
        let err = base.build_request(3, "old").expect_err("nothing to edit");
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        assert!(err.to_string().contains("--title"), "{err}");

        // Title-only edit keeps the current content, bases on the fetched
        // version.
        let args = EditArgs {
            title: Some("New title".into()),
            ..base
        };
        let request = args.build_request(3, "old content").expect("builds");
        assert_eq!(request.title.as_deref(), Some("New title"));
        assert_eq!(request.content, "old content");
        assert_eq!(request.version, 3);

        // Content + explicit --version (the stale-409 lever).
        let args = EditArgs {
            short_code: "ACME-T-0001".into(),
            title: None,
            content: Some("new content".into()),
            content_file: None,
            version: Some(1),
            common: Common::default(),
        };
        let request = args.build_request(4, "old").expect("builds");
        assert_eq!(request.title, None);
        assert_eq!(request.content, "new content");
        assert_eq!(request.version, 1);
    }

    /// KAIROS-T-0607: the lines of `documents move` and `tasks lane` come
    /// from the answer of the server, with no read before the call.
    #[test]
    fn the_move_of_a_document_and_the_lane_name_the_result() {
        assert_eq!(
            document_moved_line("ACME-D-0001", "b-2"),
            "The owner board of the document ACME-D-0001 is b-2."
        );
        assert_eq!(
            lane_line("ACME-T-0001", "planned"),
            "ACME-T-0001 is in the planned lane."
        );
    }

    /// KAIROS-T-0555: `documents lifecycle` names the lifecycle after the
    /// call, and clap refuses a value that is not one of the four.
    #[test]
    fn the_lifecycle_of_a_document_names_the_result() {
        use clap::Parser;
        #[derive(Parser, Debug)]
        struct Cli {
            #[command(subcommand)]
            command: DocumentsCommand,
        }
        assert_eq!(
            lifecycle_line("ACME-D-0001", "published"),
            "The document ACME-D-0001 is published."
        );
        let parsed =
            Cli::try_parse_from(["documents", "lifecycle", "ACME-D-0001", "--to", "review"])
                .expect("parses");
        match parsed.command {
            DocumentsCommand::Lifecycle(args) => {
                assert_eq!(args.short_code, "ACME-D-0001");
                assert_eq!(args.to, "review");
            }
            other => panic!("not the lifecycle verb: {other:?}"),
        }
        let refused =
            Cli::try_parse_from(["documents", "lifecycle", "ACME-D-0001", "--to", "final"])
                .expect_err("an unknown state is refused");
        let text = refused.to_string();
        assert!(
            text.contains("final") && text.contains("published"),
            "{text}"
        );
    }

    /// `initiatives complexity` takes a size or `none`, and clap refuses
    /// any other value.
    #[test]
    fn the_complexity_of_an_initiative_names_the_result() {
        use clap::Parser;
        #[derive(Parser, Debug)]
        struct Cli {
            #[command(subcommand)]
            command: InitiativesCommand,
        }
        assert_eq!(
            complexity_line("ACME-I-0001", Some("l")),
            "ACME-I-0001 has the complexity l."
        );
        assert_eq!(
            complexity_line("ACME-I-0001", None),
            "ACME-I-0001 has no complexity."
        );
        for value in ["l", "none"] {
            let parsed = Cli::try_parse_from(["initiatives", "complexity", "ACME-I-0001", value])
                .expect("parses");
            match parsed.command {
                InitiativesCommand::Complexity(args) => {
                    assert_eq!(args.short_code, "ACME-I-0001");
                    assert_eq!(args.complexity, value);
                }
                other => panic!("not the complexity verb: {other:?}"),
            }
        }
        let refused = Cli::try_parse_from(["initiatives", "complexity", "ACME-I-0001", "xxl"])
            .expect_err("an unknown size is refused");
        let text = refused.to_string();
        assert!(text.contains("xxl") && text.contains("none"), "{text}");
    }

    /// KAIROS-T-0589: `transition` names the column, not its id.
    #[test]
    fn the_transition_line_names_the_column() {
        let detail: kairos_client::types_org::BoardDetail =
            serde_json::from_value(serde_json::json!({
                "id": "b-1", "name": "Platform", "slug": "platform-delivery",
                "board_level": "delivery", "team_id": null, "code_prefix": "PLAT",
                "created_at": "2026-10-18T00:00:00Z", "updated_at": "2026-10-18T00:00:00Z",
                "columns": [{
                    "id": "c-2", "board_id": "b-1", "name": "Todo", "position": 1,
                    "created_at": "2026-10-18T00:00:00Z", "updated_at": "2026-10-18T00:00:00Z"
                }],
                "transitions": []
            }))
            .expect("a board detail");
        assert_eq!(
            transitioned_line("task", "ACME-T-0001", Some("c-2"), &detail),
            "Kairos moved the task ACME-T-0001 to the column Todo."
        );
        assert_eq!(
            transitioned_line("task", "ACME-T-0001", Some("c-9"), &detail),
            "Kairos moved the task ACME-T-0001 to the column c-9."
        );
    }

    /// KAIROS-T-0589: `--content` wins; with neither flag there is no text.
    #[test]
    fn the_text_comes_from_the_flag_or_the_file() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("body.md");
        std::fs::write(&path, "from a file").expect("a file");
        assert_eq!(
            read_text(Some("inline"), None, "--content-file").expect("text"),
            Some("inline".to_string())
        );
        assert_eq!(
            read_text(None, Some(&path), "--content-file").expect("file"),
            Some("from a file".to_string())
        );
        assert_eq!(read_text(None, None, "--content-file").expect("none"), None);
        let missing = dir.path().join("missing.md");
        let err = read_text(None, Some(&missing), "--content-file").expect_err("no file");
        assert!(err.to_string().contains("missing.md"), "{err}");
    }

    /// COLLIERY-T-0269: an archived repository is marked.
    #[test]
    fn the_impacts_of_an_item_are_the_slugs_of_the_repositories() {
        use kairos_client::types_repositories::ImpactedRepository;
        let link = |slug: &str, archived_at: Option<&str>| Impact {
            relationship: "impacts".into(),
            target_kind: "repository".into(),
            repository: ImpactedRepository {
                id: "r-1".into(),
                slug: slug.into(),
                forge: "github".into(),
                repo_full_name: format!("acme/{slug}"),
                archived_at: archived_at.map(str::to_string),
            },
            created_at: "2026-09-29T00:00:00Z".into(),
        };
        assert_eq!(impacts_cell(&[]), "-");
        assert_eq!(
            impacts_cell(&[
                link("fidius", None),
                link("old", Some("2026-09-01T00:00:00Z"))
            ]),
            "fidius, old [archived]"
        );
    }

    /// Deletes refuse to run without --confirm.
    /// KAIROS-T-0572: the line of `set-parent` says what changed.
    #[test]
    fn set_parent_names_the_parent_now_and_before() {
        let response = |parent: Option<&str>, previous: Option<&str>, changed| SetParentResponse {
            short_code: "ACME-T-0001".into(),
            parent: parent.map(str::to_string),
            previous_parent: previous.map(str::to_string),
            changed,
        };
        assert_eq!(
            parent_line(&response(Some("ACME-I-0002"), Some("ACME-I-0001"), true)),
            "The parent of ACME-T-0001 is ACME-I-0002 (was ACME-I-0001)."
        );
        assert_eq!(
            parent_line(&response(None, Some("ACME-I-0001"), true)),
            "ACME-T-0001 has no parent now (was ACME-I-0001)."
        );
        assert_eq!(
            parent_line(&response(Some("ACME-I-0001"), Some("ACME-I-0001"), false)),
            "ACME-T-0001 has the parent ACME-I-0001 already. Nothing changed."
        );
        assert_eq!(
            parent_line(&response(None, None, false)),
            "ACME-T-0001 has no parent. Nothing changed."
        );
    }

    #[test]
    fn delete_requires_confirm() {
        let err = require_confirm(false, "ACME-T-0001").expect_err("must refuse");
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        assert!(err.to_string().contains("--confirm"), "{err}");
        require_confirm(true, "ACME-T-0001").expect("confirmed passes");
    }

    /// KAIROS-T-0362: the guard of `archive` says that a restore brings
    /// the item back; the guard of `purge` says that nothing does.
    #[test]
    fn archive_and_purge_guards_say_what_the_verb_does() {
        let err = require_archive_confirm(false, "ACME-T-0001").expect_err("must refuse");
        assert!(err.to_string().contains("archive"), "{err}");
        assert!(err.to_string().contains("restore"), "{err}");
        require_archive_confirm(true, "ACME-T-0001").expect("confirmed passes");
        let err = require_purge_confirm(false, "ACME-T-0001").expect_err("must refuse");
        assert!(err.to_string().contains("cannot undo"), "{err}");
        require_purge_confirm(true, "ACME-T-0001").expect("confirmed passes");
    }

    /// KAIROS-T-0557: `history` prints the versions, then the activity,
    /// with names, the agent mark, the time to the second, and a line for
    /// the rows that the list does not show.
    #[test]
    fn history_prints_the_versions_and_the_activity_with_names() {
        let versions = ListEnvelope {
            items: vec![HistoryVersion {
                version: 2,
                edited_by: "u-1".to_string(),
                edited_by_name: Some("Robin".to_string()),
                edited_at: "2026-10-17T10:00:00.123456Z".to_string(),
                agent_key_id: Some("k-1".to_string()),
            }],
            total: 2,
            limit: 1,
            offset: 0,
        };
        let row = |text: &str, name: Option<&str>| ItemActivityEntry {
            id: "a".to_string(),
            action: "transition".to_string(),
            details: String::new(),
            text: text.to_string(),
            actor_id: "u-2".to_string(),
            actor_name: name.map(str::to_string),
            agent_key_id: None,
            occurred_at: "2026-10-17T11:00:00Z".to_string(),
        };
        let activity = ListEnvelope {
            items: vec![
                row("claim taken: Sam", Some("Sam")),
                row("moved from Todo to Active", None),
            ],
            total: 2,
            limit: 20,
            offset: 0,
        };
        assert_eq!(
            history_text("ACME-T-0001", &versions, &activity),
            "# History of ACME-T-0001\n\
             - v2 — 2026-10-17T10:00:00Z by Robin (agent)\n\
             - 1 older row is not shown. Use a larger --limit to see it.\n\
             \n## Activity\n\
             - claim taken: Sam — 2026-10-17T11:00:00Z by Sam\n\
             - moved from Todo to Active — 2026-10-17T11:00:00Z by u-2\n"
        );

        let none = ListEnvelope {
            items: Vec::new(),
            total: 0,
            limit: 20,
            offset: 0,
        };
        assert!(
            history_text("ACME-T-0001", &versions, &none)
                .ends_with("## Activity\nNo activity is recorded for this item.\n")
        );
    }

    /// A task from the JSON of the server, with `extra` fields.
    fn task_from(extra: serde_json::Value) -> Task {
        let mut task = serde_json::json!({
            "id": "t-1", "short_code": "ACME-T-0001", "title": "A task", "content": "",
            "board_id": "b-1", "column_id": "c-1", "task_type": "task",
            "work_class": "planned", "team_id": null, "version": 1,
            "created_by": "u-1", "updated_by": "u-1",
            "created_at": "2026-10-18T00:00:00Z", "updated_at": "2026-10-18T00:00:00Z"
        });
        for (key, value) in extra.as_object().expect("an object") {
            task[key] = value.clone();
        }
        serde_json::from_value(task).expect("a task")
    }

    fn field(task: &Task, label: &str) -> String {
        task.fields()
            .into_iter()
            .find(|(name, _)| *name == label)
            .map(|(_, value)| value)
            .unwrap_or_else(|| panic!("no field {label}"))
    }

    /// KAIROS-T-0591: `tasks show` names the repository by its slug, or `-`.
    #[test]
    fn tasks_show_names_the_repository() {
        assert_eq!(field(&task_from(serde_json::json!({})), "repository"), "-");
        let bound = task_from(serde_json::json!({
            "repository_id": "r-1",
            "repository": {
                "id": "r-1", "slug": "web", "forge": "github",
                "repo_full_name": "acme/web", "team_id": "team-1"
            }
        }));
        assert_eq!(field(&bound, "repository"), "web");
        let id_only = task_from(serde_json::json!({ "repository_id": "r-1" }));
        assert_eq!(field(&id_only, "repository"), "r-1");
    }

    fn related(code: &str, done: bool, cancelled: bool, archived: bool) -> RelatedItem {
        RelatedItem {
            relationship_id: format!("e-{code}"),
            id: format!("id-{code}"),
            short_code: code.to_string(),
            entity_type: "task".into(),
            title: format!("title {code}"),
            archived_at: archived.then(|| "2026-10-01T00:00:00Z".to_string()),
            done,
            cancelled,
        }
    }

    fn group(
        relationship: &str,
        items: Vec<RelatedItem>,
    ) -> kairos_client::types_meta::RelationshipGroup {
        kairos_client::types_meta::RelationshipGroup {
            relationship: relationship.into(),
            items,
        }
    }

    /// KAIROS-T-0591: the relationships of `show` read from the side of the
    /// item, parent first, with the marks of each neighbour.
    #[test]
    fn show_lists_the_relationships_from_the_side_of_the_item() {
        let relationships = ItemRelationshipsResponse {
            short_code: "ACME-T-0001".into(),
            outgoing: vec![
                group(
                    "supports",
                    vec![related("ACME-D-0005", false, false, false)],
                ),
                group("blocks", vec![related("ACME-T-0004", false, false, true)]),
            ],
            incoming: vec![
                group(
                    "blocks",
                    vec![
                        related("ACME-T-0003", true, false, false),
                        related("ACME-T-0006", true, true, false),
                    ],
                ),
                group("parent", vec![related("ACME-I-0002", false, false, false)]),
            ],
        };
        assert_eq!(
            relationship_lines(&relationships),
            vec![
                "  relationships:",
                "    parent: ACME-I-0002 — title ACME-I-0002",
                "    blocked by: ACME-T-0003 — title ACME-T-0003 [done]",
                "    blocked by: ACME-T-0006 — title ACME-T-0006 [cancelled]",
                "    blocks: ACME-T-0004 — title ACME-T-0004 [archived]",
                "    supported by: ACME-D-0005 — title ACME-D-0005",
            ]
        );
        let none = ItemRelationshipsResponse {
            short_code: "ACME-T-0001".into(),
            outgoing: vec![],
            incoming: vec![],
        };
        assert_eq!(relationship_lines(&none), vec!["  relationships: -"]);
        assert_eq!(relationship_label("parent", true).1, "child");
        assert_eq!(relationship_label("supports", false).1, "supports");
        assert_eq!(relationship_label("informs", false).1, "informed by");
        assert_eq!(relationship_label("supersedes", false).1, "superseded by");
        assert_eq!(relationship_label("amends", false).1, "amended by");
        assert_eq!(relationship_label("amends", true).1, "amends");
    }

    /// KAIROS-T-0591: the metadata and the external references of `show`.
    #[test]
    fn show_lists_the_metadata_and_the_external_references() {
        let value = MetadataValue {
            definition_id: "d-1".into(),
            slug: "priority".into(),
            name: "Priority".into(),
            field_type: "enum".into(),
            value: "high".into(),
        };
        assert_eq!(
            metadata_lines(&[value]),
            vec!["  metadata:", "    priority: high"]
        );
        assert_eq!(metadata_lines(&[]), vec!["  metadata: -"]);
        let reference = kairos_client::types_meta::ExternalRef {
            id: "r-1".into(),
            relationship: "external".into(),
            uri: "https://example.com/rfc".into(),
            label: Some("RFC 42".into()),
            created_by: "u-1".into(),
            created_at: "2026-10-15T00:00:00Z".into(),
        };
        assert_eq!(
            external_lines(&[reference]),
            vec!["  external:", "    RFC 42 <https://example.com/rfc>"]
        );
        assert_eq!(external_lines(&[]), vec!["  external: -"]);
    }

    /// KAIROS-T-0591: `metadata set` takes SLUG=VALUE pairs; a bad pair is
    /// a usage error that names it. `metadata unset` takes slugs.
    #[test]
    fn metadata_set_and_unset_parse_their_arguments() {
        use clap::Parser;
        #[derive(Parser, Debug)]
        struct Cli {
            #[command(subcommand)]
            command: TasksCommand,
        }
        let parsed = Cli::try_parse_from([
            "tasks",
            "metadata",
            "set",
            "ACME-T-0001",
            "priority=high",
            "note=a=b",
        ])
        .expect("parses");
        match parsed.command {
            TasksCommand::Metadata(ItemMetadataCommand::Set(args)) => {
                assert_eq!(args.short_code, "ACME-T-0001");
                assert_eq!(
                    args.values,
                    vec![
                        ("priority".to_string(), "high".to_string()),
                        ("note".to_string(), "a=b".to_string()),
                    ]
                );
            }
            other => panic!("not metadata set: {other:?}"),
        }
        let parsed = Cli::try_parse_from(["tasks", "metadata", "unset", "ACME-T-0001", "priority"])
            .expect("parses");
        match parsed.command {
            TasksCommand::Metadata(ItemMetadataCommand::Unset(args)) => {
                assert_eq!(args.slugs, vec!["priority".to_string()]);
            }
            other => panic!("not metadata unset: {other:?}"),
        }
        for (bad, says) in [
            ("priority", "has no `=`"),
            ("=high", "has no slug"),
            ("priority=", "has no value"),
        ] {
            let refused = Cli::try_parse_from(["tasks", "metadata", "set", "ACME-T-0001", bad])
                .expect_err("a bad pair");
            assert_eq!(refused.kind(), clap::error::ErrorKind::ValueValidation);
            assert!(refused.to_string().contains(says), "{refused}");
        }
        // At least one value.
        assert!(Cli::try_parse_from(["tasks", "metadata", "set", "ACME-T-0001"]).is_err());
        assert!(Cli::try_parse_from(["tasks", "metadata", "unset", "ACME-T-0001"]).is_err());
    }

    /// KAIROS-T-0591: the body of `metadata set` and `unset`; a slug given
    /// twice is refused and named.
    #[test]
    fn the_metadata_body_has_each_slug_once() {
        let body = metadata_request([("priority", Some("high")), ("due", None)]).expect("body");
        assert_eq!(body.values.get("priority"), Some(&Some("high".to_string())));
        assert_eq!(body.values.get("due"), Some(&None));
        let refused =
            metadata_request([("priority", Some("high")), ("priority", None)]).expect_err("twice");
        assert_eq!(refused.exit_code(), EXIT_FAILURE);
        assert!(refused.to_string().contains("priority"), "{refused}");
    }

    /// KAIROS-T-0591: `adrs create --parent` sends the parent; without it
    /// the body has no `parent` key, for an older server.
    #[test]
    fn adrs_create_sends_the_parent() {
        use clap::Parser;
        #[derive(Parser, Debug)]
        struct Cli {
            #[command(subcommand)]
            command: AdrsCommand,
        }
        let parsed = Cli::try_parse_from([
            "adrs",
            "create",
            "--title",
            "T",
            "--board",
            "adrs",
            "--parent",
            "ACME-I-0002",
        ])
        .expect("parses");
        match parsed.command {
            AdrsCommand::Create(args) => assert_eq!(args.parent.as_deref(), Some("ACME-I-0002")),
            other => panic!("not create: {other:?}"),
        }
        let body = CreateAdrRequest {
            board_id: None,
            column_id: None,
            title: "T".into(),
            content: String::new(),
            decision_maker: None,
            decision_date: None,
            parent: None,
        };
        let json = serde_json::to_value(&body).expect("json");
        assert!(json.get("parent").is_none(), "{json}");
    }
}
