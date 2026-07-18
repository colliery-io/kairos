//! `kairos search` — the S-0005 unified-search body composed from flags
//! (common filters, traverse) or supplied verbatim via `--query-json`
//! (KAIROS-A-0007 / KAIROS-T-0037).

use std::collections::BTreeMap;
use std::io::Read;

use clap::Args;

use kairos_client::types_search::{
    SearchFilter, SearchRequest, SearchResponse, SearchSort, SearchTraverse, SearchTraverseFrom,
};

use crate::commands::entities::EntityView;
use crate::context::{Common, client, print_json};
use crate::error::CliError;
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
    /// Restrict to items on this board (UUID)
    #[arg(long, value_name = "BOARD_ID")]
    pub board: Option<String>,
    /// Restrict to items in this column (UUID)
    #[arg(long, value_name = "COLUMN_ID")]
    pub column: Option<String>,
    /// Restrict to tasks assigned to this team (UUID)
    #[arg(long, value_name = "TEAM_ID")]
    pub team: Option<String>,
    /// Task type filter, repeatable: task|bug|tech_debt
    #[arg(long, value_name = "TASK_TYPE")]
    pub task_type: Vec<String>,
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
    /// Include soft-deleted items
    #[arg(long)]
    pub include_deleted: bool,
    /// Traverse: starting entity's short code
    #[arg(long, value_name = "SHORT_CODE", conflicts_with = "from_id")]
    pub from: Option<String>,
    /// Traverse: starting entity's id (UUID)
    #[arg(long, value_name = "UUID")]
    pub from_id: Option<String>,
    /// Traverse: relationship types to follow, repeatable:
    /// parent|supports|informs|supersedes|blocks
    #[arg(long, value_name = "REL")]
    pub relationships: Vec<String>,
    /// Traverse: edge direction: outbound|inbound|both (defaults to
    /// outbound)
    #[arg(long, value_name = "DIRECTION")]
    pub direction: Option<String>,
    /// Traverse: maximum depth (1-10)
    #[arg(long, value_name = "N")]
    pub depth: Option<u32>,
    /// Sort field: created_at|updated_at|title (defaults to created_at)
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
    /// The full S-0005 search body as raw JSON (`@FILE` reads a file, `-`
    /// reads stdin); cannot be combined with the other search flags
    #[arg(long, value_name = "JSON")]
    pub query_json: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

impl SearchArgs {
    /// Whether any flag other than `--query-json` (and the pagination
    /// defaults) was supplied.
    fn has_flag_query(&self) -> bool {
        self.query.is_some()
            || !self.entity_type.is_empty()
            || self.board.is_some()
            || self.column.is_some()
            || self.team.is_some()
            || !self.task_type.is_empty()
            || self.is_bucket.is_some()
            || !self.metadata.is_empty()
            || self.after.is_some()
            || self.before.is_some()
            || self.include_deleted
            || self.from.is_some()
            || self.from_id.is_some()
            || !self.relationships.is_empty()
            || self.direction.is_some()
            || self.depth.is_some()
            || self.sort.is_some()
            || self.order.is_some()
            || self.limit.is_some()
            || self.offset.is_some()
    }

    /// Compose the S-0005 request body from the flags (or take
    /// `--query-json` verbatim).
    pub fn build_request(&self) -> Result<SearchRequest, CliError> {
        if let Some(raw) = &self.query_json {
            if self.has_flag_query() {
                return Err(CliError::Failure(
                    "--query-json carries the full search body and cannot be combined \
                     with the other search flags"
                        .to_string(),
                ));
            }
            let text = if raw == "-" {
                let mut text = String::new();
                std::io::stdin()
                    .read_to_string(&mut text)
                    .map_err(|err| CliError::Failure(format!("cannot read stdin: {err}")))?;
                text
            } else if let Some(path) = raw.strip_prefix('@') {
                std::fs::read_to_string(path)
                    .map_err(|err| CliError::Failure(format!("cannot read {path}: {err}")))?
            } else {
                raw.clone()
            };
            return serde_json::from_str(&text).map_err(|err| {
                CliError::Failure(format!(
                    "--query-json is not a valid S-0005 search body: {err}"
                ))
            });
        }

        let metadata = self.parse_metadata()?;
        let has_filter = !self.entity_type.is_empty()
            || self.board.is_some()
            || self.column.is_some()
            || self.team.is_some()
            || !self.task_type.is_empty()
            || self.is_bucket.is_some()
            || metadata.is_some()
            || self.after.is_some()
            || self.before.is_some()
            || self.include_deleted;
        let filter = has_filter.then(|| SearchFilter {
            entity_type: non_empty(&self.entity_type),
            board_id: self.board.clone(),
            column_id: self.column.clone(),
            team_id: self.team.clone(),
            task_type: non_empty(&self.task_type),
            is_bucket: self.is_bucket,
            metadata,
            created_after: self.after.clone(),
            created_before: self.before.clone(),
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
                "nothing to search for: pass --query, a filter flag, --from/--from-id, \
                 or --query-json"
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
                    "--metadata takes KEY=VALUE pairs, got {pair:?}"
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
                    "--relationships/--direction/--depth describe a traversal and require \
                     --from or --from-id"
                        .to_string(),
                ));
            }
            return Ok(None);
        }
        if self.relationships.is_empty() {
            return Err(CliError::Failure(
                "traversal requires at least one --relationships \
                 (parent|supports|informs|supersedes|blocks)"
                    .to_string(),
            ));
        }
        if self.depth.is_none() {
            return Err(CliError::Failure(
                "traversal requires --depth (1-10)".to_string(),
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
        let request = self.build_request()?;
        let client = client(&self.common)?;
        let response = client.search(&request).await?;
        if self.common.json {
            return print_json(&response);
        }
        print_results(&response);
        Ok(())
    }
}

fn non_empty(values: &[String]) -> Option<Vec<String>> {
    (!values.is_empty()).then(|| values.to_vec())
}

/// The human rendering: one CODE/TITLE/VER section per non-empty group.
fn print_results(response: &SearchResponse) {
    fn section<T: EntityView>(label: &str, items: &[T]) {
        if items.is_empty() {
            return;
        }
        println!("{label}:");
        let mut table = Table::new(&["  CODE", "TITLE", "VER"]);
        for item in items {
            table.row(vec![
                format!("  {}", item.short_code()),
                item.title().to_string(),
                item.version().to_string(),
            ]);
        }
        print!("{}", table.render());
    }

    if response.total == 0 {
        println!("(no matches)");
    } else {
        section("strategies", &response.results.strategies);
        section("initiatives", &response.results.initiatives);
        section("tasks", &response.results.tasks);
        section("documents", &response.results.documents);
        section("adrs", &response.results.adrs);
    }
    println!(
        "total: {} (limit {}, offset {})",
        response.total, response.limit, response.offset
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(filter.board_id.as_deref(), Some("b-1"));
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

    /// --query-json is the verbatim escape hatch: parsed as the full
    /// S-0005 body, rejected when combined with flags or malformed, and an
    /// empty flag set with no query at all is a usage error.
    #[test]
    fn query_json_escape_hatch() {
        let args = SearchArgs {
            query_json: Some(r#"{"q": "auth", "limit": 5}"#.into()),
            ..Default::default()
        };
        let request = args.build_request().expect("parses");
        assert_eq!(request.q.as_deref(), Some("auth"));
        assert_eq!(request.limit, Some(5));

        let err = SearchArgs {
            query: Some("auth".into()),
            query_json: Some(r#"{"q": "auth"}"#.into()),
            ..Default::default()
        }
        .build_request()
        .expect_err("flags + --query-json conflict");
        assert!(err.to_string().contains("cannot be combined"), "{err}");

        let err = SearchArgs {
            query_json: Some(r#"{"query": "typo"}"#.into()),
            ..Default::default()
        }
        .build_request()
        .expect_err("unknown fields rejected");
        assert!(err.to_string().contains("not a valid"), "{err}");

        let err = SearchArgs::default()
            .build_request()
            .expect_err("empty search");
        assert!(err.to_string().contains("nothing to search"), "{err}");

        let err = SearchArgs {
            metadata: vec!["no-equals".into()],
            ..Default::default()
        }
        .build_request()
        .expect_err("bad metadata pair");
        assert!(err.to_string().contains("KEY=VALUE"), "{err}");
    }
}
