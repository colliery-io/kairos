//! `kairos search` — the S-0005 unified-search body composed from flags
//! (common filters, traverse) or supplied verbatim via `--query-json`
//! (KAIROS-A-0007 / KAIROS-T-0037).

use std::collections::BTreeMap;
use std::io::Read;

use clap::Args;

use kairos_client::types_cards::{AdrCard, DocumentCard, InitiativeCard, StrategyCard, TaskCard};
use kairos_client::types_search::{
    SearchFilter, SearchRequest, SearchResultGroups, SearchSort, SearchTraverse, SearchTraverseFrom,
};
use serde::Serialize;

use crate::context::{Common, client};
use crate::error::CliError;
use crate::listing::{ALL_PAGE, Listing, read_each_page};
use crate::table::Table;

/// Search and traverse all entity types (POST /api/search).
#[derive(Args, Debug, Default)]
pub struct SearchArgs {
    /// Full-text query (websearch semantics: quoted phrases, OR,
    /// -negation)
    #[arg(long, short = 'q')]
    pub query: Option<String>,
    /// Entity type filter, repeatable:
    /// strategy|initiative|task|document|adr
    #[arg(long = "type", value_name = "ENTITY_TYPE")]
    pub entity_type: Vec<String>,
    /// Restrict to items on this board (slug or UUID)
    #[arg(long, value_name = "BOARD")]
    pub board: Option<String>,
    /// Restrict to items in this column: a name (with --board) or a UUID
    #[arg(long, value_name = "COLUMN")]
    pub column: Option<String>,
    /// Restrict to tasks assigned to this team (slug or UUID)
    #[arg(long, value_name = "TEAM")]
    pub team: Option<String>,
    /// Restrict to the items of this repository (slug or UUID): the tasks
    /// that link to it, and the documents and the ADRs that impact it
    #[arg(long, value_name = "REPOSITORY")]
    pub repo: Option<String>,
    /// The kind of a task, repeatable: task|bug|tech_debt|support. The old
    /// name --task-type still works
    #[arg(long = "kind", alias = "task-type", value_name = "KIND")]
    pub task_type: Vec<String>,
    /// Planned/Support lane filter, repeatable: planned|support
    /// (KAIROS-T-0077)
    #[arg(long, value_name = "WORK_CLASS")]
    pub work_class: Vec<String>,
    /// Restrict to (non-)bucket initiatives
    #[arg(long, value_name = "BOOL")]
    pub is_bucket: Option<bool>,
    /// Metadata condition `slug=value`, repeatable (values support
    /// trailing-* globs, e.g. component=auth*)
    #[arg(long, value_name = "KEY=VALUE")]
    pub metadata: Vec<String>,
    /// Only items created strictly after this instant (RFC 3339)
    #[arg(long, value_name = "RFC3339")]
    pub after: Option<String>,
    /// Only items created strictly before this instant (RFC 3339)
    #[arg(long, value_name = "RFC3339")]
    pub before: Option<String>,
    /// Only items last changed strictly after this instant (RFC 3339)
    #[arg(long, value_name = "RFC3339")]
    pub updated_after: Option<String>,
    /// Only items last changed strictly before this instant (RFC 3339),
    /// e.g. 90 days ago for the items untouched for 90 days
    #[arg(long, value_name = "RFC3339")]
    pub updated_before: Option<String>,
    /// Only items with no value for this metadata definition (slug),
    /// repeatable, e.g. --metadata-missing priority
    #[arg(long, value_name = "SLUG")]
    pub metadata_missing: Vec<String>,
    /// Include archived (soft-deleted) items; composes with --query and
    /// --from, and archived hits are marked
    #[arg(long)]
    pub include_deleted: bool,
    /// Only tasks that this person has the claim on, also through an
    /// agent key: a user id, an email, a user name or `me`
    #[arg(long, value_name = "PERSON")]
    pub claimed_by: Option<String>,
    /// Only strategies and initiatives that this person owns: a user id,
    /// an email, a user name or `me`
    #[arg(long, value_name = "PERSON")]
    pub owned_by: Option<String>,
    /// Only items that this person created: a user id, an email, a user
    /// name or `me`
    #[arg(long, value_name = "PERSON")]
    pub created_by: Option<String>,
    /// Traverse: starting entity's short code
    #[arg(long, value_name = "SHORT_CODE", conflicts_with = "from_id")]
    pub from: Option<String>,
    /// Traverse: starting entity's id (UUID)
    #[arg(long, value_name = "UUID")]
    pub from_id: Option<String>,
    /// Traverse: relationship types to follow, repeatable:
    /// parent|supports|informs|supersedes|amends|blocks (required with --from)
    #[arg(long, value_name = "REL")]
    pub relationships: Vec<String>,
    /// Traverse: edge direction: outbound|inbound|both (defaults to
    /// outbound)
    #[arg(long, value_name = "DIRECTION")]
    pub direction: Option<String>,
    /// Traverse: maximum depth, 1-10 (required with --from)
    #[arg(long, value_name = "N")]
    pub depth: Option<u32>,
    /// Sort field: created_at|updated_at|title|relevance (defaults to
    /// relevance when --query is given, created_at otherwise; relevance
    /// needs --query)
    #[arg(long, value_name = "FIELD")]
    pub sort: Option<String>,
    /// Sort order: asc|desc (defaults to desc)
    #[arg(long, value_name = "ORDER")]
    pub order: Option<String>,
    /// Page size (server default 25, max 100)
    #[arg(long)]
    pub limit: Option<i64>,
    /// Offset into the combined result set
    #[arg(long)]
    pub offset: Option<i64>,
    /// Read each page: the CLI asks for one page after the other, and
    /// prints each hit. Not with --limit, --offset or --query-json
    #[arg(long, conflicts_with_all = ["limit", "offset"])]
    pub all: bool,
    /// The full S-0005 search body as raw JSON (`@FILE` reads a file, `-`
    /// reads stdin); cannot be combined with the other search flags
    #[arg(long, value_name = "JSON")]
    pub query_json: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl SearchArgs {
    /// The search flags on the command line, by name: each argument but
    /// `--query-json` and the common options (KAIROS-T-0593). A unit test
    /// sets each clap argument in turn, so a new flag cannot be left out.
    fn flags_given(&self) -> Vec<&'static str> {
        [
            (self.query.is_some(), "--query"),
            (!self.entity_type.is_empty(), "--type"),
            (self.board.is_some(), "--board"),
            (self.column.is_some(), "--column"),
            (self.team.is_some(), "--team"),
            (self.repo.is_some(), "--repo"),
            (!self.task_type.is_empty(), "--kind"),
            (!self.work_class.is_empty(), "--work-class"),
            (self.is_bucket.is_some(), "--is-bucket"),
            (!self.metadata.is_empty(), "--metadata"),
            (self.after.is_some(), "--after"),
            (self.before.is_some(), "--before"),
            (self.updated_after.is_some(), "--updated-after"),
            (self.updated_before.is_some(), "--updated-before"),
            (!self.metadata_missing.is_empty(), "--metadata-missing"),
            (self.include_deleted, "--include-deleted"),
            (self.claimed_by.is_some(), "--claimed-by"),
            (self.owned_by.is_some(), "--owned-by"),
            (self.created_by.is_some(), "--created-by"),
            (self.from.is_some(), "--from"),
            (self.from_id.is_some(), "--from-id"),
            (!self.relationships.is_empty(), "--relationships"),
            (self.direction.is_some(), "--direction"),
            (self.depth.is_some(), "--depth"),
            (self.sort.is_some(), "--sort"),
            (self.order.is_some(), "--order"),
            (self.limit.is_some(), "--limit"),
            (self.offset.is_some(), "--offset"),
            (self.all, "--all"),
        ]
        .into_iter()
        .filter_map(|(given, name)| given.then_some(name))
        .collect()
    }

    /// The body of the search: `--query-json` as it is, or the body that
    /// the flags compose. `--query-json` with a search flag is refused.
    pub fn build_body(&self) -> Result<SearchBody, CliError> {
        let Some(raw) = &self.query_json else {
            return Ok(SearchBody::Flags(Box::new(self.build_request()?)));
        };
        let given = self.flags_given();
        if !given.is_empty() {
            return Err(CliError::Failure(format!(
                "--query-json has the full search body. Do not use it with {}. Put the \
                 values in the JSON, or use the flags without --query-json.",
                given.join(", ")
            )));
        }
        let text = if raw == "-" {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).map_err(|err| {
                CliError::Failure(format!("The CLI cannot read standard input: {err}."))
            })?;
            text
        } else if let Some(path) = raw.strip_prefix('@') {
            std::fs::read_to_string(path)
                .map_err(|err| CliError::Failure(format!("The CLI cannot read {path}: {err}.")))?
        } else {
            raw.clone()
        };
        // The body goes to the server as it is: the server judges each
        // field, so an older CLI sends a field that a newer server knows.
        let body: serde_json::Value = serde_json::from_str(&text).map_err(|err| {
            CliError::Failure(format!("The value of --query-json is not JSON: {err}."))
        })?;
        if !body.is_object() {
            return Err(CliError::Failure(
                "The value of --query-json is not a JSON object. The search body is an object."
                    .to_string(),
            ));
        }
        Ok(SearchBody::Raw(body))
    }

    /// Compose the S-0005 request body from the flags.
    pub fn build_request(&self) -> Result<SearchRequest, CliError> {
        let metadata = self.parse_metadata()?;
        let has_filter = !self.entity_type.is_empty()
            || self.board.is_some()
            || self.column.is_some()
            || self.team.is_some()
            || self.repo.is_some()
            || !self.task_type.is_empty()
            || !self.work_class.is_empty()
            || self.is_bucket.is_some()
            || metadata.is_some()
            || self.after.is_some()
            || self.before.is_some()
            || self.updated_after.is_some()
            || self.updated_before.is_some()
            || !self.metadata_missing.is_empty()
            || self.claimed_by.is_some()
            || self.owned_by.is_some()
            || self.created_by.is_some()
            || self.include_deleted;
        let filter = has_filter.then(|| SearchFilter {
            entity_type: non_empty(&self.entity_type),
            board_id: None,
            board: self.board.clone(),
            column_id: None,
            column: self.column.clone(),
            team_id: None,
            team: self.team.clone(),
            repository: self.repo.clone(),
            task_type: non_empty(&self.task_type),
            work_class: non_empty(&self.work_class),
            is_bucket: self.is_bucket,
            metadata,
            created_after: self.after.clone(),
            created_before: self.before.clone(),
            updated_after: self.updated_after.clone(),
            updated_before: self.updated_before.clone(),
            metadata_missing: non_empty(&self.metadata_missing),
            claimed_by: self.claimed_by.clone(),
            owned_by: self.owned_by.clone(),
            created_by: self.created_by.clone(),
            include_deleted: self.include_deleted,
        });

        let traverse = self.build_traverse()?;
        let sort = (self.sort.is_some() || self.order.is_some()).then(|| SearchSort {
            field: self
                .sort
                .clone()
                .unwrap_or_else(|| "created_at".to_string()),
            order: self.order.clone().unwrap_or_else(|| "desc".to_string()),
        });

        if self.query.is_none() && filter.is_none() && traverse.is_none() {
            return Err(CliError::Failure(
                "The command has no search. Use --query, a filter flag, --from, --from-id or \
                 --query-json."
                    .to_string(),
            ));
        }

        Ok(SearchRequest {
            q: self.query.clone(),
            filter,
            traverse,
            sort,
            limit: self.limit,
            offset: self.offset,
        })
    }

    /// `--metadata k=v` pairs into the S-0005 metadata map.
    fn parse_metadata(&self) -> Result<Option<BTreeMap<String, String>>, CliError> {
        if self.metadata.is_empty() {
            return Ok(None);
        }
        let mut map = BTreeMap::new();
        for pair in &self.metadata {
            let Some((key, value)) = pair.split_once('=') else {
                return Err(CliError::Failure(format!(
                    "The value {pair:?} of --metadata is not correct. Write it as KEY=VALUE."
                )));
            };
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
        Ok(Some(map))
    }

    /// The traverse clause: `--from`/`--from-id` anchor it; the companion
    /// flags are rejected without an anchor, and an anchor requires
    /// `--relationships` and `--depth` (S-0005 requires both).
    fn build_traverse(&self) -> Result<Option<SearchTraverse>, CliError> {
        if self.from.is_none() && self.from_id.is_none() {
            if !self.relationships.is_empty() || self.direction.is_some() || self.depth.is_some() {
                return Err(CliError::Failure(
                    "--relationships, --direction and --depth are for a traversal. Use them \
                     with --from or --from-id."
                        .to_string(),
                ));
            }
            return Ok(None);
        }
        if self.relationships.is_empty() {
            return Err(CliError::Failure(
                "A traversal needs one --relationships or more. The values are: parent, \
                 supports, informs, supersedes, amends, blocks."
                    .to_string(),
            ));
        }
        if self.depth.is_none() {
            return Err(CliError::Failure(
                "A traversal needs --depth. The depth is a number from 1 to 10.".to_string(),
            ));
        }
        Ok(Some(SearchTraverse {
            from: SearchTraverseFrom {
                short_code: self.from.clone(),
                id: self.from_id.clone(),
            },
            relationships: self.relationships.clone(),
            direction: self
                .direction
                .clone()
                .unwrap_or_else(|| "outbound".to_string()),
            depth: self.depth,
        }))
    }

    pub async fn run(self) -> Result<(), CliError> {
        let body = self.build_body()?;
        let client = client(&self.common)?;
        let listing = match (&body, self.all) {
            // `--all` beside `--query-json` is refused in `build_body`.
            (SearchBody::Flags(request), true) => {
                let (items, total) = read_each_page(|offset| {
                    let page = SearchRequest {
                        limit: Some(ALL_PAGE),
                        offset: Some(offset),
                        ..(**request).clone()
                    };
                    let client = &client;
                    async move {
                        let response = client.search(&page).await?;
                        Ok::<_, CliError>((hits(response.results), response.total))
                    }
                })
                .await?;
                Listing {
                    items,
                    total,
                    limit: ALL_PAGE,
                    offset: 0,
                    all: true,
                }
            }
            (body, _) => {
                let response = match body {
                    SearchBody::Flags(request) => client.search(request).await?,
                    SearchBody::Raw(body) => client.search_raw(body).await?,
                };
                Listing {
                    items: hits(response.results),
                    total: response.total,
                    limit: response.limit,
                    offset: response.offset,
                    all: false,
                }
            }
        };
        if self.common.json {
            return listing.print_json();
        }
        print_results(&listing);
        Ok(())
    }
}

/// One hit of a search in the `--json` list (KAIROS-A-0057): the card,
/// with its `entity_type`. A box serializes as its card.
#[derive(Debug, Serialize)]
#[serde(tag = "entity_type", rename_all = "lowercase")]
pub enum Hit {
    Strategy(Box<StrategyCard>),
    Initiative(Box<InitiativeCard>),
    Task(Box<TaskCard>),
    Document(Box<DocumentCard>),
    Adr(Box<AdrCard>),
}

impl Hit {
    /// The fields of the table.
    fn row(&self) -> &dyn HitRow {
        match self {
            Self::Strategy(card) => card.as_ref(),
            Self::Initiative(card) => card.as_ref(),
            Self::Task(card) => card.as_ref(),
            Self::Document(card) => card.as_ref(),
            Self::Adr(card) => card.as_ref(),
        }
    }

    /// The group of the hit, in the order of the groups.
    fn group(&self) -> usize {
        match self {
            Self::Strategy(_) => 0,
            Self::Initiative(_) => 1,
            Self::Task(_) => 2,
            Self::Document(_) => 3,
            Self::Adr(_) => 4,
        }
    }
}

/// The labels of the groups, in the order of [`Hit::group`].
const GROUPS: [&str; 5] = ["strategies", "initiatives", "tasks", "documents", "adrs"];

/// The hits of one page as one list, in the order of the groups. Pure.
pub fn hits(results: SearchResultGroups) -> Vec<Hit> {
    let SearchResultGroups {
        strategies,
        initiatives,
        tasks,
        documents,
        adrs,
    } = results;
    strategies
        .into_iter()
        .map(|card| Hit::Strategy(Box::new(card)))
        .chain(
            initiatives
                .into_iter()
                .map(|card| Hit::Initiative(Box::new(card))),
        )
        .chain(tasks.into_iter().map(|card| Hit::Task(Box::new(card))))
        .chain(
            documents
                .into_iter()
                .map(|card| Hit::Document(Box::new(card))),
        )
        .chain(adrs.into_iter().map(|card| Hit::Adr(Box::new(card))))
        .collect()
}

/// The body of a search: composed from the flags, or `--query-json` as it
/// is.
#[derive(Debug)]
pub enum SearchBody {
    Flags(Box<SearchRequest>),
    Raw(serde_json::Value),
}

fn non_empty(values: &[String]) -> Option<Vec<String>> {
    (!values.is_empty()).then(|| values.to_vec())
}

/// The fields of a hit that the table shows (KAIROS-T-0602): a hit is a
/// card, not the item.
trait HitRow {
    fn short_code(&self) -> &str;
    fn title(&self) -> &str;
    fn version(&self) -> i32;
    fn archived_at(&self) -> Option<&str>;
}

macro_rules! hit_row {
    ($($type:ty),*) => {$(
        impl HitRow for $type {
            fn short_code(&self) -> &str {
                &self.short_code
            }
            fn title(&self) -> &str {
                &self.title
            }
            fn version(&self) -> i32 {
                self.version
            }
            fn archived_at(&self) -> Option<&str> {
                self.archived_at.as_deref()
            }
        }
    )*};
}

hit_row!(
    kairos_client::types_cards::StrategyCard,
    kairos_client::types_cards::InitiativeCard,
    kairos_client::types_cards::TaskCard,
    kairos_client::types_cards::DocumentCard,
    kairos_client::types_cards::AdrCard
);

/// The human rendering: one CODE/TITLE/VER section per non-empty group.
/// An archived hit carries an `[archived]` marker on its title
/// (KAIROS-A-0020, KAIROS-T-0157): `--include-deleted` mixes put-away work
/// in with live work, and a reader who cannot tell them apart will act on
/// the wrong one.
fn print_results(listing: &Listing<Hit>) {
    print!("{}", render_results(listing));
}

/// [`print_results`] as text. Pure.
fn render_results(listing: &Listing<Hit>) -> String {
    let mut out = String::new();
    if listing.total == 0 {
        out.push_str("(no matches)\n");
    }
    for (group, label) in GROUPS.iter().enumerate() {
        let rows: Vec<&dyn HitRow> = listing
            .items
            .iter()
            .filter(|hit| hit.group() == group)
            .map(Hit::row)
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("{label}:\n"));
        let mut table = Table::new(&["  CODE", "TITLE", "VER"]);
        for item in rows {
            let title = match item.archived_at() {
                Some(_) => format!("{} [archived]", item.title()),
                None => item.title().to_string(),
            };
            table.row(vec![
                format!("  {}", item.short_code()),
                title,
                item.version().to_string(),
            ]);
        }
        out.push_str(&table.render());
    }
    out.push_str(&listing.footer());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of search results with one strategy and two tasks.
    fn groups() -> SearchResultGroups {
        let card = |code: &str, archived: bool| {
            serde_json::json!({
                "id": format!("id-{code}"),
                "short_code": code,
                "title": format!("Title {code}"),
                "board_id": "b",
                "column_id": "c",
                "task_type": "task",
                "work_class": "planned",
                "team_id": null,
                "version": 1,
                "created_at": "2026-10-18T00:00:00Z",
                "updated_at": "2026-10-18T00:00:00Z",
                "archived_at": archived.then_some("2026-10-18T00:00:00Z"),
            })
        };
        serde_json::from_value(serde_json::json!({
            "tasks": [card("ACME-T-0001", false), card("ACME-T-0002", true)],
            "strategies": [card("ACME-S-0001", false)],
        }))
        .expect("search groups")
    }

    /// KAIROS-A-0057: the hits are one list in the order of the groups,
    /// and each hit names its entity type.
    #[test]
    fn the_hits_are_one_list_with_the_entity_type() {
        let hits = hits(groups());
        let json = serde_json::to_value(&hits).unwrap();
        let rows: Vec<(String, String)> = json
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| {
                (
                    hit["entity_type"].as_str().unwrap().to_string(),
                    hit["short_code"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("strategy".to_string(), "ACME-S-0001".to_string()),
                ("task".to_string(), "ACME-T-0001".to_string()),
                ("task".to_string(), "ACME-T-0002".to_string()),
            ]
        );
        let listing = Listing {
            items: hits,
            total: 5,
            limit: 3,
            offset: 0,
            all: false,
        };
        let envelope = serde_json::to_value(listing.json()).unwrap();
        assert_eq!(envelope["total"], 5);
        assert_eq!(envelope["next_offset"], 3);
    }

    /// The text groups the flat list again, marks an archived hit, and ends
    /// with the footer.
    #[test]
    fn the_text_groups_the_hits() {
        let listing = Listing {
            items: hits(groups()),
            total: 3,
            limit: ALL_PAGE,
            offset: 0,
            all: true,
        };
        let text = render_results(&listing);
        assert!(text.starts_with("strategies:\n"), "{text}");
        assert!(text.contains("tasks:\n"), "{text}");
        assert!(text.contains("Title ACME-T-0002 [archived]"), "{text}");
        assert!(text.ends_with("total: 3\n"), "{text}");
        let none = Listing::<Hit> {
            items: vec![],
            total: 0,
            limit: 25,
            offset: 0,
            all: false,
        };
        assert_eq!(
            render_results(&none),
            "(no matches)\ntotal: 0 (limit 25, offset 0)\n"
        );
    }

    /// `--all` is a usage error beside `--limit` or `--offset`, and is
    /// refused beside `--query-json`.
    #[test]
    fn all_is_not_with_limit_offset_or_query_json() {
        use clap::Parser;

        #[derive(Parser, Debug)]
        struct Harness {
            #[command(flatten)]
            search: SearchArgs,
        }
        for extra in [["--limit", "5"], ["--offset", "5"]] {
            let argv = ["search", "--query", "x", "--all", extra[0], extra[1]];
            assert!(Harness::try_parse_from(argv).is_err(), "{argv:?}");
        }
        let parsed = Harness::try_parse_from(["search", "--query-json", "{}", "--all"]).unwrap();
        let err = parsed.search.build_body().unwrap_err();
        assert!(err.to_string().contains("--all"), "{err}");
    }

    /// The filter flags compose the S-0005 body: metadata k=v parsing,
    /// repeated --type/--task-type, timestamps, sort defaults.
    #[test]
    fn flags_compose_the_s0005_body() {
        let args = SearchArgs {
            query: Some("auth".into()),
            entity_type: vec!["task".into(), "initiative".into()],
            board: Some("b-1".into()),
            task_type: vec!["bug".into()],
            metadata: vec!["priority=critical".into(), "component=auth*".into()],
            after: Some("2026-01-01T00:00:00Z".into()),
            before: Some("2026-03-01T00:00:00Z".into()),
            updated_before: Some("2026-02-01T00:00:00Z".into()),
            metadata_missing: vec!["priority".into()],
            sort: Some("updated_at".into()),
            limit: Some(10),
            ..Default::default()
        };
        let request = args.build_request().expect("builds");
        assert_eq!(request.q.as_deref(), Some("auth"));
        let filter = request.filter.expect("filter");
        assert_eq!(
            filter.entity_type.as_deref(),
            Some(&["task".to_string(), "initiative".to_string()][..])
        );
        // KAIROS-T-0368: --board is a slug or a UUID, sent as `board`.
        assert_eq!(filter.board.as_deref(), Some("b-1"));
        assert_eq!(filter.board_id, None);
        assert_eq!(
            filter.updated_before.as_deref(),
            Some("2026-02-01T00:00:00Z")
        );
        assert_eq!(
            filter.metadata_missing.as_deref(),
            Some(&["priority".to_string()][..])
        );
        assert_eq!(filter.task_type.as_deref(), Some(&["bug".to_string()][..]));
        let metadata = filter.metadata.expect("metadata");
        assert_eq!(metadata["priority"], "critical");
        assert_eq!(metadata["component"], "auth*");
        assert_eq!(
            filter.created_after.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert!(!filter.include_deleted);
        let sort = request.sort.expect("sort");
        assert_eq!(
            (sort.field.as_str(), sort.order.as_str()),
            ("updated_at", "desc")
        );
        assert_eq!(request.limit, Some(10));
        assert!(request.traverse.is_none());
    }

    /// KAIROS-T-0488: the person flags are filters on their own, and go
    /// to the body as given.
    #[test]
    fn person_flags_compose_the_filter() {
        let args = SearchArgs {
            claimed_by: Some("me".into()),
            owned_by: Some("robin".into()),
            created_by: Some("sam@example.test".into()),
            ..Default::default()
        };
        let filter = args
            .build_request()
            .expect("builds")
            .filter
            .expect("filter");
        assert_eq!(filter.claimed_by.as_deref(), Some("me"));
        assert_eq!(filter.owned_by.as_deref(), Some("robin"));
        assert_eq!(filter.created_by.as_deref(), Some("sam@example.test"));
    }

    /// Traversal flags: anchored by --from, direction defaults to
    /// outbound; companions without an anchor (or an anchor without
    /// --relationships/--depth) are usage errors.
    #[test]
    fn traverse_flags() {
        let args = SearchArgs {
            from: Some("ACME-S-0001".into()),
            relationships: vec!["parent".into()],
            depth: Some(3),
            ..Default::default()
        };
        let request = args.build_request().expect("builds");
        let traverse = request.traverse.expect("traverse");
        assert_eq!(traverse.from.short_code.as_deref(), Some("ACME-S-0001"));
        assert_eq!(traverse.direction, "outbound");
        assert_eq!(traverse.depth, Some(3));

        let err = SearchArgs {
            relationships: vec!["parent".into()],
            ..Default::default()
        }
        .build_request()
        .expect_err("companions need an anchor");
        assert!(err.to_string().contains("--from"), "{err}");

        let err = SearchArgs {
            from: Some("ACME-S-0001".into()),
            depth: Some(2),
            ..Default::default()
        }
        .build_request()
        .expect_err("anchor needs relationships");
        assert!(err.to_string().contains("--relationships"), "{err}");

        let err = SearchArgs {
            from: Some("ACME-S-0001".into()),
            relationships: vec!["parent".into()],
            ..Default::default()
        }
        .build_request()
        .expect_err("anchor needs depth");
        assert!(err.to_string().contains("--depth"), "{err}");
    }

    /// `kairos search --repo <slug>` on its own is a complete request
    /// (KAIROS-T-0119 UAT finding: the repository filter was left out of
    /// the "is there a filter at all" check and the documented invocation
    /// was refused as "nothing to search for").
    #[test]
    fn repo_alone_is_a_filter() {
        let request = SearchArgs {
            repo: Some("payments-api".into()),
            ..SearchArgs::default()
        }
        .build_request()
        .expect("--repo alone is a filter");
        assert_eq!(
            request.filter.expect("filter").repository.as_deref(),
            Some("payments-api")
        );
    }

    /// --query-json is the verbatim escape hatch: sent as it is, with a
    /// field that the client does not know; refused with a flag, or when it
    /// is not a JSON object (KAIROS-T-0593).
    #[test]
    fn query_json_escape_hatch() {
        let body = r#"{"q": "auth", "limit": 5, "a_newer_field": true}"#;
        let args = SearchArgs {
            query_json: Some(body.into()),
            ..Default::default()
        };
        let SearchBody::Raw(sent) = args.build_body().expect("parses") else {
            panic!("--query-json is sent as it is");
        };
        assert_eq!(
            sent,
            serde_json::from_str::<serde_json::Value>(body).unwrap()
        );

        let err = SearchArgs {
            query: Some("auth".into()),
            updated_after: Some("2026-01-01T00:00:00Z".into()),
            query_json: Some(r#"{"q": "auth"}"#.into()),
            ..Default::default()
        }
        .build_body()
        .expect_err("flags + --query-json conflict");
        assert!(
            err.to_string()
                .contains("Do not use it with --query, --updated-after."),
            "{err}"
        );

        for bad in ["[1, 2]", "not json"] {
            let err = SearchArgs {
                query_json: Some(bad.into()),
                ..Default::default()
            }
            .build_body()
            .expect_err("not an object");
            assert!(err.to_string().contains("--query-json is not"), "{err}");
        }

        let err = SearchArgs::default()
            .build_request()
            .expect_err("empty search");
        assert!(
            err.to_string().contains("The command has no search"),
            "{err}"
        );

        let err = SearchArgs {
            metadata: vec!["no-equals".into()],
            ..Default::default()
        }
        .build_request()
        .expect_err("bad metadata pair");
        assert!(err.to_string().contains("KEY=VALUE"), "{err}");
    }

    /// Each argument of `kairos search` but `--query-json` and the common
    /// options is refused beside `--query-json`, and the refusal names it
    /// (KAIROS-T-0593: three flags were dropped without a word).
    #[test]
    fn each_search_flag_is_refused_beside_query_json() {
        use clap::{Args, CommandFactory, Parser};

        #[derive(Parser, Debug)]
        struct Harness {
            #[command(flatten)]
            search: SearchArgs,
        }

        let common: Vec<String> = Common::augment_args(clap::Command::new("common"))
            .get_arguments()
            .map(|a| a.get_id().to_string())
            .collect();
        let mut checked = 0;
        for arg in Harness::command().get_arguments() {
            let id = arg.get_id().to_string();
            if ["query_json", "help", "version"].contains(&id.as_str()) || common.contains(&id) {
                continue;
            }
            let long = format!("--{}", arg.get_long().expect("a long flag"));
            let mut argv = vec![
                "search".to_string(),
                "--query-json".into(),
                "{}".into(),
                long.clone(),
            ];
            if arg.get_action().takes_values() {
                argv.push("1".into());
            }
            let parsed = Harness::try_parse_from(&argv).or_else(|_| {
                argv.pop();
                argv.push("true".into());
                Harness::try_parse_from(&argv)
            });
            let parsed = parsed.unwrap_or_else(|err| panic!("{long}: {err}"));
            let err = parsed
                .search
                .build_body()
                .expect_err("a flag beside --query-json is refused");
            assert!(
                err.to_string()
                    .contains(&format!("Do not use it with {long}.")),
                "{long}: {err}"
            );
            checked += 1;
        }
        assert!(checked >= 28, "only {checked} flags checked");
    }
}
