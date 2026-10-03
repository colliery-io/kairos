//! The five S-0005 entity families as CLI nouns (KAIROS-T-0037 /
//! KAIROS-A-0015): `strategies|initiatives|tasks|documents|adrs`, each with
//! `list/get/create/edit/transition/delete` (documents have no transition
//! endpoint). A thin veneer over `kairos-client` — the only CLI-side logic
//! is flag plumbing, the fetch-then-patch edit flow, and rendering.

use std::io::Read;
use std::path::{Path, PathBuf};

use clap::Args;

use kairos_client::types::{
    Adr, CreateAdrRequest, CreateDocumentRequest, CreateInitiativeRequest, CreateStrategyRequest,
    CreateTaskRequest, DeleteResponse, Document, ImpactListQuery, Initiative, ListEnvelope,
    ListQuery, Pagination, RestoreResponse, Strategy, Task, UpdateContentRequest,
};
use kairos_client::types_repositories::Impact;

use crate::context::{Common, client, print_json};
use crate::error::CliError;
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
    #[command(flatten)]
    pub common: Common,
}

impl ListArgs {
    pub fn page(&self) -> Pagination {
        Pagination {
            limit: self.limit,
            offset: self.offset,
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
    #[command(flatten)]
    pub common: Common,
}

impl EntityListArgs {
    pub fn query(&self) -> ListQuery {
        ListQuery {
            limit: self.limit,
            offset: self.offset,
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
    #[command(flatten)]
    pub common: Common,
}

impl ImpactListArgs {
    pub fn query(&self) -> ImpactListQuery {
        ImpactListQuery {
            limit: self.limit,
            offset: self.offset,
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

impl EditArgs {
    /// The PATCH body: flags merged over the fetched current entity.
    pub fn build_request(
        &self,
        current_version: i32,
        current_content: &str,
    ) -> Result<UpdateContentRequest, CliError> {
        let content = match (&self.content, &self.content_file) {
            (Some(content), _) => Some(content.clone()),
            (None, Some(path)) if path == Path::new("-") => {
                let mut text = String::new();
                std::io::stdin().read_to_string(&mut text).map_err(|err| {
                    CliError::Failure(format!("The CLI cannot read standard input: {err}."))
                })?;
                Some(text)
            }
            (None, Some(path)) => Some(std::fs::read_to_string(path).map_err(|err| {
                CliError::Failure(format!("The CLI cannot read {}: {err}.", path.display()))
            })?),
            (None, None) => None,
        };
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
    /// Target column id (UUID); a move outside the board's transition
    /// graph is rejected with 422 listing the allowed targets
    #[arg(long = "to", value_name = "COLUMN_ID")]
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

/// Arguments of `kairos documents move` (COLLIERY-T-0269).
#[derive(Args, Debug)]
pub struct DocumentMoveArgs {
    /// The document's short code (e.g. ACME-D-0001)
    pub short_code: String,
    /// The new owner board (slug or UUID), of each level. The owner board
    /// gives the right to edit the document. You need `manage_documents`
    /// on the board that owns the document now and on this board
    #[arg(
        long = "to-board",
        value_name = "BOARD",
        required_unless_present = "no_board",
        conflicts_with = "no_board"
    )]
    pub to_board: Option<String>,
    /// Remove the owner board. The owner is then the board of the item
    /// that the document supports. A document that supports no item keeps
    /// its board
    #[arg(long = "no-board")]
    pub no_board: bool,
    /// Also give the document the next code of its new owner board. The
    /// old code is retired, and each reference to it in the text of the
    /// items changes to the new code. It needs --to-board
    #[arg(long, conflicts_with = "no_board")]
    pub rename: bool,
    #[command(flatten)]
    pub common: Common,
}

/// Arguments of the `delete` verbs (soft delete, KAIROS-A-0001 cascade).
#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// The item's short code (e.g. ACME-T-0001)
    pub short_code: String,
    /// Actually delete (the soft delete cascades to children)
    #[arg(long)]
    pub confirm: bool,
    #[command(flatten)]
    pub common: Common,
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
    fn column_id(&self) -> Option<&str> {
        Some(&self.column_id)
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
    fn column_id(&self) -> Option<&str> {
        Some(&self.column_id)
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
    fn column_id(&self) -> Option<&str> {
        Some(&self.column_id)
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
    fn column_id(&self) -> Option<&str> {
        None
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
            // COLLIERY-T-0269. The board that the document names. With
            // `-`, the owner is the board of the item that it supports.
            ("owner board", or_dash(&self.board_id)),
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
    fn column_id(&self) -> Option<&str> {
        self.column_id.as_deref()
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

pub fn emit_list<T: EntityView>(
    common: &Common,
    envelope: &ListEnvelope<T>,
) -> Result<(), CliError> {
    if common.json {
        return print_json(envelope);
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
    println!(
        "total: {} (limit {}, offset {})",
        envelope.total, envelope.limit, envelope.offset
    );
    Ok(())
}

pub fn emit_get<T: EntityView>(common: &Common, item: &T) -> Result<(), CliError> {
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
    println!();
    println!("{}", item.content());
    Ok(())
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

pub fn emit_transitioned<T: EntityView>(common: &Common, item: &T) -> Result<(), CliError> {
    if common.json {
        return print_json(item);
    }
    println!(
        "Kairos moved the {} {} to the column {}.",
        T::NOUN,
        item.short_code(),
        item.column_id().unwrap_or("-")
    );
    Ok(())
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
pub fn emit_document_moved(
    common: &Common,
    before: Option<&str>,
    document: &Document,
) -> Result<(), CliError> {
    if common.json {
        return print_json(document);
    }
    println!(
        "{}",
        document_moved_line(&document.short_code, before, document.board_id.as_deref())
    );
    Ok(())
}

/// The line of [`emit_document_moved`].
fn document_moved_line(short_code: &str, before: Option<&str>, after: Option<&str>) -> String {
    match (before, after) {
        (before, after) if before == after => match after {
            Some(board) => format!(
                "Kairos did not change the document {short_code}. Its owner board is {board} \
                 already."
            ),
            None => {
                format!("Kairos did not change the document {short_code}. It names no owner board.")
            }
        },
        (_, Some(board)) => {
            format!("Kairos moved the document {short_code} to the owner board {board}.")
        }
        (_, None) => format!(
            "Kairos removed the owner board of the document {short_code}. Its owner is the \
             board of the item that it supports."
        ),
    }
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
        let reason = match (&item.below, &item.required_capability, &item.board_id) {
            (Some(stop), _, _) => format!("It is below {stop}."),
            (None, Some(capability), Some(board)) => {
                format!("You need {capability} on the board {board}.")
            }
            (None, Some(capability), None) => {
                format!("It has no board for {capability}. Ask an organization admin.")
            }
            (None, None, _) => "You cannot edit it.".to_string(),
        };
        lines.push(format!("    - {}: {reason}", item.short_code));
    }
    lines
}

// ---------------------------------------------------------------------------
// Per-family create arguments
// ---------------------------------------------------------------------------

/// Arguments of `kairos strategies create`.
#[derive(Args, Debug)]
pub struct StrategyCreateArgs {
    /// Board to create the strategy on (slug or UUID)
    #[arg(long, value_name = "BOARD")]
    pub board: String,
    /// Column to place it in (UUID; defaults to the board's first column)
    #[arg(long, value_name = "COLUMN_ID")]
    pub column: Option<String>,
    /// Title
    #[arg(long)]
    pub title: String,
    /// Markdown content (defaults to empty)
    #[arg(long, default_value = "")]
    pub content: String,
    /// Strategy hypothesis
    #[arg(long)]
    pub hypothesis: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl StrategyCreateArgs {
    fn request(&self) -> CreateStrategyRequest {
        CreateStrategyRequest {
            board_id: self.board.clone(),
            column_id: self.column.clone(),
            title: self.title.clone(),
            content: self.content.clone(),
            hypothesis: self.hypothesis.clone(),
        }
    }
}

/// Arguments of `kairos initiatives create`.
#[derive(Args, Debug)]
pub struct InitiativeCreateArgs {
    /// Board to create the initiative on (slug or UUID)
    #[arg(long, value_name = "BOARD")]
    pub board: String,
    /// Column to place it in (UUID; defaults to the board's first column)
    #[arg(long, value_name = "COLUMN_ID")]
    pub column: Option<String>,
    /// Title
    #[arg(long)]
    pub title: String,
    /// Markdown content (defaults to empty)
    #[arg(long, default_value = "")]
    pub content: String,
    /// T-shirt sizing: xs|s|m|l|xl
    #[arg(long)]
    pub complexity: Option<String>,
    /// Mark as a bucket of this kind: tech_debt|bug|ad_hoc
    #[arg(long, value_name = "KIND")]
    pub bucket_type: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl InitiativeCreateArgs {
    fn request(&self) -> CreateInitiativeRequest {
        CreateInitiativeRequest {
            board_id: self.board.clone(),
            column_id: self.column.clone(),
            title: self.title.clone(),
            content: self.content.clone(),
            complexity: self.complexity.clone(),
            bucket_type: self.bucket_type.clone(),
        }
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
    /// Column to place it in (UUID; defaults to the board's first column)
    #[arg(long, value_name = "COLUMN_ID")]
    pub column: Option<String>,
    /// Title
    #[arg(long)]
    pub title: String,
    /// Markdown content (defaults to empty)
    #[arg(long, default_value = "")]
    pub content: String,
    /// Task type: task|bug|tech_debt|support (defaults to task)
    #[arg(long = "type", value_name = "TASK_TYPE")]
    pub task_type: Option<String>,
    /// Planned/Support lane: planned|support (KAIROS-T-0077; defaults to
    /// support for support-type tasks, else planned)
    #[arg(long = "work-class", value_name = "WORK_CLASS")]
    pub work_class: Option<String>,
    /// Team (UUID). Without --board, the task goes to the delivery board
    /// of this team. With --board, it must be the team of that board
    #[arg(long, value_name = "TEAM_ID")]
    pub team: Option<String>,
    /// Repository the task links to (slug or UUID). It can be any
    /// repository, of any team. It does not choose the board
    #[arg(long, value_name = "REPOSITORY")]
    pub repo: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl TaskCreateArgs {
    fn request(&self) -> CreateTaskRequest {
        CreateTaskRequest {
            board_id: self.board.clone(),
            repository: self.repo.clone(),
            column_id: self.column.clone(),
            title: self.title.clone(),
            content: self.content.clone(),
            task_type: self.task_type.clone(),
            work_class: self.work_class.clone(),
            team_id: self.team.clone(),
        }
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
    /// `manage_documents` on it. The document is not a card of the board.
    /// A document must have --board, or --parent, or the two
    #[arg(long, value_name = "BOARD", required_unless_present = "parent")]
    pub board: Option<String>,
    /// Short code of the workflow item this document supports. With no
    /// --board, the document takes the board of that item as its owner
    #[arg(long, value_name = "SHORT_CODE")]
    pub parent: Option<String>,
    /// Markdown content (omit with --template to stamp the template's
    /// content)
    #[arg(long)]
    pub content: Option<String>,
    /// Template to stamp content + metadata defaults from (UUID)
    #[arg(long, value_name = "TEMPLATE_ID")]
    pub template: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl DocumentCreateArgs {
    fn request(&self) -> CreateDocumentRequest {
        CreateDocumentRequest {
            title: self.title.clone(),
            board: self.board.clone(),
            content: self.content.clone(),
            template_id: self.template.clone(),
            parent_short_code: self.parent.clone(),
        }
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
    /// Column to place it in (UUID; defaults to the board's first column)
    #[arg(long, value_name = "COLUMN_ID")]
    pub column: Option<String>,
    /// Markdown content (defaults to empty)
    #[arg(long, default_value = "")]
    pub content: String,
    /// Decision maker
    #[arg(long)]
    pub decision_maker: Option<String>,
    /// Decision date (YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    pub decision_date: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl AdrCreateArgs {
    fn request(&self) -> CreateAdrRequest {
        CreateAdrRequest {
            board_id: self.board.clone(),
            column_id: self.column.clone(),
            title: self.title.clone(),
            content: self.content.clone(),
            decision_maker: self.decision_maker.clone(),
            decision_date: self.decision_date.clone(),
        }
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
/// owner board (COLLIERY-T-0269). `list_args` is the type of the flags of
/// `list`: [`ImpactListArgs`] for documents and ADRs, which have the
/// filter `--repo`.
macro_rules! entity_family_cli {
    (
        $enum_name:ident, $noun:literal, $create_args:ty, list_args = $list_args:ty,
        list = $list:ident, get = $get:ident, create = $create_fn:ident,
        update = $update:ident, delete = $delete:ident, restore = $restore:ident
        $(, transition($transition_variant:ident) = $transition_fn:ident)?
        $(, board_move($move_variant:ident) = $move_fn:ident)?
        $(, owner_move($owner_variant:ident) = $owner_fn:ident)?
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
                                      ", including its markdown content"))]
            Get(GetArgs),
            #[command(about = concat!("Create a ", $noun))]
            Create($create_args),
            #[command(about = concat!("Edit a ", $noun, "'s title and/or content ",
                                      "(version-checked; a concurrent edit gets 409)"))]
            Edit(EditArgs),
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
            #[command(about = concat!("Move a ", $noun, " to another owner board, ",
                                      "or remove its owner board (--no-board)"))]
            $owner_variant(DocumentMoveArgs),
            )?
            #[command(about = concat!("Soft-delete a ", $noun,
                                      " and cascade to its children (requires --confirm)"))]
            Delete(DeleteArgs),
            #[command(about = concat!("Put an archived ", $noun,
                                      " back on its board (its archived children stay archived)"))]
            Restore(GetArgs),
        }

        impl $enum_name {
            pub async fn run(self) -> Result<(), CliError> {
                match self {
                    Self::List(args) => {
                        let client = client(&args.common)?;
                        let envelope = client.$list(args.query()).await?;
                        emit_list(&args.common, &envelope)
                    }
                    Self::Get(args) => {
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
                        emit_get(&args.common, &item)
                    }
                    Self::Create(args) => {
                        let client = client(&args.common)?;
                        let item = client.$create_fn(&args.request()).await?;
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
                    $(
                    Self::$transition_variant(args) => {
                        let client = client(&args.common)?;
                        let item = client
                            .$transition_fn(&args.short_code, &args.to_column)
                            .await?;
                        emit_transitioned(&args.common, &item)
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
                        // The board before the call, to say what changed:
                        // the response of a call that changes nothing is a
                        // normal 200.
                        let before = client.$get(&args.short_code).await?.board_id;
                        let item = client
                            .$owner_fn(&args.short_code, args.to_board.as_deref(), args.rename)
                            .await?;
                        emit_document_moved(&args.common, before.as_deref(), &item)?;
                        if !args.common.json {
                            if let Some(line) = renamed_line(&args.short_code, &item.short_code) {
                                println!("{line}");
                            }
                        }
                        Ok(())
                    }
                    )?
                    Self::Delete(args) => {
                        require_confirm(args.confirm, &args.short_code)?;
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
    transition(Transition) = transition_initiative
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
    transition(Transition) = transition_task,
    board_move(Move) = move_task_with
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
    owner_move(Move) = set_document_board_with
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
    transition(Transition) = transition_adr
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EXIT_FAILURE;

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

    /// COLLIERY-T-0269: the line of `documents move` says what changed.
    #[test]
    fn the_move_of_a_document_says_what_changed() {
        assert_eq!(
            document_moved_line("ACME-D-0001", None, Some("b-1")),
            "Kairos moved the document ACME-D-0001 to the owner board b-1."
        );
        assert_eq!(
            document_moved_line("ACME-D-0001", Some("b-1"), Some("b-2")),
            "Kairos moved the document ACME-D-0001 to the owner board b-2."
        );
        assert_eq!(
            document_moved_line("ACME-D-0001", Some("b-1"), None),
            "Kairos removed the owner board of the document ACME-D-0001. Its owner is the \
             board of the item that it supports."
        );
        assert_eq!(
            document_moved_line("ACME-D-0001", Some("b-1"), Some("b-1")),
            "Kairos did not change the document ACME-D-0001. Its owner board is b-1 already."
        );
        assert_eq!(
            document_moved_line("ACME-D-0001", None, None),
            "Kairos did not change the document ACME-D-0001. It names no owner board."
        );
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
    #[test]
    fn delete_requires_confirm() {
        let err = require_confirm(false, "ACME-T-0001").expect_err("must refuse");
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        assert!(err.to_string().contains("--confirm"), "{err}");
        require_confirm(true, "ACME-T-0001").expect("confirmed passes");
    }
}
