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
    CreateTaskRequest, DeleteResponse, Document, Initiative, ListEnvelope, Pagination, Strategy,
    Task, UpdateContentRequest,
};

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
                std::io::stdin()
                    .read_to_string(&mut text)
                    .map_err(|err| CliError::Failure(format!("cannot read stdin: {err}")))?;
                Some(text)
            }
            (None, Some(path)) => Some(std::fs::read_to_string(path).map_err(|err| {
                CliError::Failure(format!("cannot read {}: {err}", path.display()))
            })?),
            (None, None) => None,
        };
        if self.title.is_none() && content.is_none() {
            return Err(CliError::Failure(
                "nothing to edit: pass --title, --content, or --content-file".to_string(),
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
            "refusing to delete {what} without --confirm"
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
    /// One `list` table row, matching [`Self::HEADERS`].
    fn table_row(&self) -> Vec<String>;
    /// `(label, value)` detail lines for `get` (content rendered
    /// separately).
    fn fields(&self) -> Vec<(&'static str, String)>;
}

fn or_dash(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "-".to_string())
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
            table.row(item.table_row());
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
        "Created {} {} (version {}): {}",
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
        "Edited {} {}: now version {}",
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
        "Transitioned {} {} to column {}",
        T::NOUN,
        item.short_code(),
        item.column_id().unwrap_or("-")
    );
    Ok(())
}

pub fn emit_deleted(common: &Common, response: &DeleteResponse) -> Result<(), CliError> {
    if common.json {
        return print_json(response);
    }
    if response.cascade_count == 0 {
        println!("Deleted {}", response.short_code);
    } else {
        println!(
            "Deleted {} (cascaded to {} descendants: {})",
            response.short_code,
            response.cascade_count,
            response.cascaded_short_codes.join(", ")
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-family create arguments
// ---------------------------------------------------------------------------

/// Arguments of `kairos strategies create`.
#[derive(Args, Debug)]
pub struct StrategyCreateArgs {
    /// Board to create the strategy on (UUID)
    #[arg(long, value_name = "BOARD_ID")]
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
    /// Board to create the initiative on (UUID)
    #[arg(long, value_name = "BOARD_ID")]
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
    /// Delivery board to create the task on (UUID). Optional when --repo
    /// is given: the task is routed to the repository's owning team's
    /// delivery board (KAIROS-A-0019)
    #[arg(long, value_name = "BOARD_ID", required_unless_present = "repo")]
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
    /// Owning team (UUID; defaults to the repository's owning team)
    #[arg(long, value_name = "TEAM_ID")]
    pub team: Option<String>,
    /// Repository to issue the task against (slug or UUID); routes the
    /// task to the owning team's delivery board
    #[arg(long, value_name = "REPOSITORY")]
    pub repo: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl TaskCreateArgs {
    fn request(&self) -> CreateTaskRequest {
        CreateTaskRequest {
            board_id: self.board.clone(),
            repository_id: self.repo.clone(),
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
    /// Short code of the workflow item this document supports (required by
    /// the API; documents inherit that item's board for authorization)
    #[arg(long, value_name = "SHORT_CODE")]
    pub parent: String,
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
            content: self.content.clone(),
            template_id: self.template.clone(),
            parent_short_code: Some(self.parent.clone()),
        }
    }
}

/// Arguments of `kairos adrs create`.
#[derive(Args, Debug)]
pub struct AdrCreateArgs {
    /// Title
    #[arg(long)]
    pub title: String,
    /// ADR board (UUID); omit for an off-board ADR (org-admin only)
    #[arg(long, value_name = "BOARD_ID")]
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
/// (documents have no transition endpoint in S-0005).
macro_rules! entity_family_cli {
    (
        $enum_name:ident, $noun:literal, $create_args:ty,
        list = $list:ident, get = $get:ident, create = $create_fn:ident,
        update = $update:ident, delete = $delete:ident
        $(, transition($transition_variant:ident) = $transition_fn:ident)?
    ) => {
        #[doc = concat!("Operations on ", $noun, "s.")]
        #[derive(clap::Subcommand, Debug)]
        pub enum $enum_name {
            // clap cannot evaluate `#[doc = concat!(..)]` (the tokens reach
            // the derive unexpanded), so per-verb help goes through
            // `command(about = ..)`, which clap emits as an expression.
            #[command(about = concat!("List ", $noun, "s (paginated)"))]
            List(ListArgs),
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
            #[command(about = concat!("Soft-delete a ", $noun,
                                      " and cascade to its children (requires --confirm)"))]
            Delete(DeleteArgs),
        }

        impl $enum_name {
            pub async fn run(self) -> Result<(), CliError> {
                match self {
                    Self::List(args) => {
                        let client = client(&args.common)?;
                        let envelope = client.$list(args.page()).await?;
                        emit_list(&args.common, &envelope)
                    }
                    Self::Get(args) => {
                        let client = client(&args.common)?;
                        let item = client.$get(&args.short_code).await?;
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
                    Self::Delete(args) => {
                        require_confirm(args.confirm, &args.short_code)?;
                        let client = client(&args.common)?;
                        let response = client.$delete(&args.short_code).await?;
                        emit_deleted(&args.common, &response)
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
    list = list_strategies,
    get = get_strategy,
    create = create_strategy,
    update = update_strategy,
    delete = delete_strategy,
    transition(Transition) = transition_strategy
);

entity_family_cli!(
    InitiativesCommand,
    "initiative",
    InitiativeCreateArgs,
    list = list_initiatives,
    get = get_initiative,
    create = create_initiative,
    update = update_initiative,
    delete = delete_initiative,
    transition(Transition) = transition_initiative
);

entity_family_cli!(
    TasksCommand,
    "task",
    TaskCreateArgs,
    list = list_tasks,
    get = get_task,
    create = create_task,
    update = update_task,
    delete = delete_task,
    transition(Transition) = transition_task
);

entity_family_cli!(
    DocumentsCommand,
    "document",
    DocumentCreateArgs,
    list = list_documents,
    get = get_document,
    create = create_document,
    update = update_document,
    delete = delete_document
);

entity_family_cli!(
    AdrsCommand,
    "ADR",
    AdrCreateArgs,
    list = list_adrs,
    get = get_adr,
    create = create_adr,
    update = update_adr,
    delete = delete_adr,
    transition(Transition) = transition_adr
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EXIT_FAILURE;

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

    /// Deletes refuse to run without --confirm.
    #[test]
    fn delete_requires_confirm() {
        let err = require_confirm(false, "ACME-T-0001").expect_err("must refuse");
        assert_eq!(err.exit_code(), EXIT_FAILURE);
        assert!(err.to_string().contains("--confirm"), "{err}");
        require_confirm(true, "ACME-T-0001").expect("confirmed passes");
    }
}
