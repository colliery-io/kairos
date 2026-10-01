//! The local MCP server of the code tools (COLLIERY-I-0264, "What an agent
//! gets"; COLLIERY-T-1852): `kairos index mcp` talks MCP over stdio and
//! reads the index of one checkout.
//!
//! Each tool call opens the index again. So a `kairos index update` that
//! runs while the server runs is seen at the next call. The results are
//! short texts (ASD-STE100), as the results of the Kairos server are.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kairos_embed::EmbeddingProvider;
use kairos_index::{
    CallEdge, DEFAULT_MIN_LINES, DuplicateKind, DuplicateOptions, Index, IndexError, Lookup,
    SearchMode, SymbolInfo,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, Implementation, ServerCapabilities,
    ServerInfo,
};
use rmcp::schemars::JsonSchema;
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::Deserialize;

use super::arguments::{Parameters, tool_error_of};
use super::error::{IndexCommandError, ToolError};

/// The results of `code_search` when the call gives no `limit`.
const DEFAULT_LIMIT: usize = 10;
/// The most results of one `code_search`.
const MAX_LIMIT: usize = 50;
/// The groups of `duplicates` when the call gives no `limit`.
pub(super) const DEFAULT_GROUPS: usize = 20;
/// The most groups of one `duplicates`.
pub(super) const MAX_GROUPS: usize = 200;

/// Serve the code tools of the checkout at `root` on stdin and stdout until
/// the client closes stdin.
pub async fn serve(root: PathBuf) -> Result<(), IndexCommandError> {
    let server = CodeTools::new(root)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| IndexCommandError::Server(e.to_string()))?;
    server
        .waiting()
        .await
        .map_err(|e| IndexCommandError::Server(e.to_string()))?;
    Ok(())
}

/// The vector provider of the queries, made on the first search that
/// needs it: the model and the provider, or why there is none.
type QueryEmbedder = (String, Result<Arc<dyn EmbeddingProvider>, String>);

#[derive(Clone)]
pub struct CodeTools {
    root: PathBuf,
    embedder: Arc<Mutex<Option<QueryEmbedder>>>,
    tool_router: ToolRouter<Self>,
}

// The arguments of each tool. `deny_unknown_fields` is the rule of the
// Kairos MCP tools: an argument that does nothing tells the agent that it
// did something.

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    /// What the code does, in plain words.
    pub query: String,
    /// The most results to give, from 1 to 50. Default 10.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SymbolParams {
    /// A name, `Type::name` (Rust) or `Type.name`.
    pub symbol: String,
    /// The file of the symbol, from the root of the checkout, or the end of
    /// its path. Give it when more than one symbol has the name.
    pub file: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct EdgeParams {
    /// A name, `Type::name` (Rust) or `Type.name`.
    pub symbol: String,
    /// The file of the symbol, when more than one symbol has the name.
    pub file: Option<String>,
    /// Also give the possible calls: calls by a name that more than one
    /// function has. Default false.
    pub possible: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct PathParams {
    /// The symbol where the chain starts.
    pub from: String,
    /// The symbol where the chain stops.
    pub to: String,
    /// The file of `from`, when more than one symbol has the name.
    pub from_file: Option<String>,
    /// The file of `to`, when more than one symbol has the name.
    pub to_file: Option<String>,
}

/// A kind of repeated code, as an argument.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub enum KindParam {
    Exact,
    Near,
    SameIdea,
}

impl From<KindParam> for DuplicateKind {
    fn from(k: KindParam) -> Self {
        match k {
            KindParam::Exact => DuplicateKind::Exact,
            KindParam::Near => DuplicateKind::Near,
            KindParam::SameIdea => DuplicateKind::SameIdea,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct DuplicatesParams {
    /// Only this kind: `exact` (the same code with other whitespace or
    /// comments), `near` (a copy with renamed names or a few changed lines) or
    /// `same-idea` (summaries with the same meaning). Default: all 3.
    pub kind: Option<KindParam>,
    /// The smallest function to compare, in lines. Default 5.
    pub min_lines: Option<u32>,
    /// Also compare test code. Default false.
    pub tests: Option<bool>,
    /// Only the functions in files under this path, from the root of the
    /// checkout.
    pub under: Option<String>,
    /// The most groups to give, from 1 to 200. Default 20.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ModuleMapParams {
    /// Only the folders under this path, from the root of the checkout.
    pub under: Option<String>,
}

impl CodeTools {
    pub fn new(root: PathBuf) -> Self {
        CodeTools {
            root,
            embedder: Arc::new(Mutex::new(None)),
            tool_router: Self::tool_router(),
        }
    }

    fn open(&self) -> Result<Index, ToolError> {
        let db = self.root.join(kairos_index::INDEX_FILE);
        if !db.is_file() {
            return Err(ToolError::NoIndex(db));
        }
        Ok(Index::open(&db)?)
    }

    /// The provider for the model of the vectors of the pool.
    fn embedder(&self, model: &str) -> Result<Arc<dyn EmbeddingProvider>, String> {
        let mut cached = self.embedder.lock().expect("the lock of the embedder");
        if let Some((cached_model, provider)) = cached.as_ref()
            && cached_model == model
        {
            return provider.clone();
        }
        let provider = super::query_embedder(model).map(Arc::from);
        *cached = Some((model.to_string(), provider.clone()));
        provider
    }
}

/// Find the symbol, or the error that says why not.
fn find(index: &Index, symbol: &str, file: Option<&str>) -> Result<SymbolInfo, ToolError> {
    match index.lookup(symbol, file)? {
        Lookup::Found(found) => Ok(found),
        Lookup::Missing => Err(ToolError::NoSymbol(match file {
            Some(file) => format!("{symbol}\" in \"{file}"),
            None => symbol.to_string(),
        })),
        Lookup::Ambiguous(all) => Err(ToolError::Ambiguous {
            name: symbol.to_string(),
            count: all.len(),
            candidates: all.iter().take(20).map(described).collect(),
        }),
    }
}

/// `Stack::pop (src/stack.rs:20)`.
fn described(s: &SymbolInfo) -> String {
    format!("{} ({})", s.qualified(), s.place())
}

fn text(t: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(t)])
}

fn error(e: ToolError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(e.text())])
}

fn result(r: Result<String, ToolError>) -> Result<CallToolResult, ErrorData> {
    Ok(match r {
        Ok(t) => text(t),
        Err(e) => error(e),
    })
}

/// The marks of an edge after its class: where it comes from when that is
/// not plain, and whether it waits for SCIP.
fn marks(edge: &CallEdge) -> String {
    let mut out = edge.class.clone();
    if edge.class == "possible" {
        out.push_str(&format!(
            ": 1 of {} functions with this name",
            edge.candidates
        ));
    }
    if edge.origin == "macro-text" {
        out.push_str(", in the text of a macro");
    }
    if edge.scip_pending {
        out.push_str(", by name until the next SCIP run");
    }
    out
}

/// The text of `duplicates`, for the MCP tool and for `kairos index
/// duplicates` (COLLIERY-T-1857): the count, what was left out, then each
/// group with its kind, its score and its symbols.
pub(super) fn duplicates_text(
    index: &Index,
    options: &DuplicateOptions,
    limit: usize,
) -> Result<String, IndexError> {
    let found = index.duplicates(options)?;
    let compared = plural(found.compared, "function", "functions");
    let mut out = if found.groups.is_empty() {
        format!("No repeated code is in the {compared} that were compared.")
    } else {
        let mut head = format!(
            "{} of repeated code in {compared}.",
            plural(found.groups.len(), "group", "groups")
        );
        if found.groups.len() > limit {
            head.push_str(&format!(
                " Only {} shown.",
                plural(limit, "group is", "groups are")
            ));
        }
        head
    };
    if !options.include_tests {
        out.push_str(" Test code is left out.");
    }
    if options.min_lines > 1 {
        out.push_str(&format!(
            " Functions under {} lines are left out.",
            options.min_lines
        ));
    }
    if options.kinds.contains(&DuplicateKind::SameIdea) && !found.summary_vectors {
        out.push_str(" The functions have no summary vectors, so no search for the same idea ran.");
    }
    for (i, g) in found.groups.iter().take(limit).enumerate() {
        out.push_str(&format!(
            "\n{}. {}, score {:.2}, {}:",
            i + 1,
            g.kind.as_str(),
            g.score,
            plural(g.symbols.len(), "symbol", "symbols")
        ));
        for s in &g.symbols {
            out.push_str(&format!(
                "\n- {} ({}:{}-{})",
                s.qualified(),
                s.file,
                s.start_line,
                s.end_line
            ));
        }
    }
    Ok(out)
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

#[tool_router]
impl CodeTools {
    #[tool(
        description = "Search the code of this checkout by what it does. Give a description in plain words. The result is the best functions and types, with their files, lines and summaries. If the index has no summaries, the search reads the names, the signatures and the paths, and the result says so."
    )]
    async fn code_search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
            if !(1..=MAX_LIMIT).contains(&limit) {
                return Err(ToolError::Limit(MAX_LIMIT));
            }
            let index = self.open()?;
            let mut why_no_vectors = None;
            let embedder = match index.vector_model()? {
                Some(model) => match self.embedder(&model) {
                    Ok(provider) => Some(provider),
                    Err(why) => {
                        why_no_vectors = Some(why);
                        None
                    }
                },
                None => None,
            };
            let found = index.search(&params.query, embedder.as_deref(), limit)?;
            let mut out = match found.mode {
                SearchMode::Vectors => {
                    "The search used the summaries and their vectors.".to_string()
                }
                SearchMode::SummaryText => format!(
                    "The search used the text of the summaries only. {}",
                    why_no_vectors.unwrap_or_else(|| "The summaries have no vectors.".into())
                ),
                SearchMode::Names => "The index has no summaries. So the search used the names, \
                     the signatures and the paths only. A build with the summarizer adds them."
                    .to_string(),
            };
            if found.hits.is_empty() {
                out.push_str(&format!("\nNo result for {:?}.", params.query));
                return Ok(out);
            }
            out.push_str(&format!(
                "\n{} for {:?}:",
                plural(found.hits.len(), "result", "results"),
                params.query
            ));
            for (i, hit) in found.hits.iter().enumerate() {
                let s = &hit.symbol;
                out.push_str(&format!("\n{}. {}, {}", i + 1, described(s), s.kind));
                match (&s.summary, &s.signature) {
                    (Some(summary), _) => out.push_str(&format!(": {summary}")),
                    (None, Some(signature)) => out.push_str(&format!(": `{signature}`")),
                    (None, None) => {}
                }
            }
            Ok(out)
        })())
    }

    #[tool(
        description = "One symbol of this checkout: its kind, file, lines, signature and summary. `symbol` is a name, `Type::name` or `Type.name`. If more than one symbol has the name, the error lists them: give `file` to choose one."
    )]
    async fn symbol(
        &self,
        Parameters(params): Parameters<SymbolParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let index = self.open()?;
            let s = find(&index, &params.symbol, params.file.as_deref())?;
            let mut out = format!(
                "{} ({}, {})\nFile: {}:{}-{}",
                s.qualified(),
                s.kind,
                s.language,
                s.file,
                s.start_line,
                s.end_line
            );
            if let Some(signature) = &s.signature {
                out.push_str(&format!("\nSignature: `{signature}`"));
            }
            match &s.summary {
                Some(summary) => out.push_str(&format!("\nSummary: {summary}")),
                None if s.is_test => out.push_str("\nSummary: none. Test code gets no summary."),
                None => out.push_str("\nSummary: none."),
            }
            if s.is_test {
                out.push_str("\nThis is test code.");
            }
            Ok(out)
        })())
    }

    #[tool(
        description = "The functions that call a symbol of this checkout. The result has the certain calls only, unless `possible` is true. A possible call is a call by a name that more than one function has: the result marks it. A call marked \"by name until the next SCIP run\" is in changed Rust code."
    )]
    async fn callers(
        &self,
        Parameters(params): Parameters<EdgeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let index = self.open()?;
            let s = find(&index, &params.symbol, params.file.as_deref())?;
            let all = index.callers(s.id, true)?;
            let possible = params.possible.unwrap_or(false);
            let (shown, hidden): (Vec<&CallEdge>, Vec<&CallEdge>) =
                all.iter().partition(|e| possible || e.class == "certain");
            let mut out = format!(
                "{} of {}:",
                plural(shown.len(), "caller", "callers"),
                described(&s)
            );
            for e in &shown {
                out.push_str(&format!(
                    "\n- {}, call at line {}, {}",
                    described(&e.other),
                    e.line,
                    marks(e)
                ));
            }
            if !hidden.is_empty() {
                out.push_str(&format!(
                    "\n{} not shown. Set `possible` to true to see them.",
                    plural(hidden.len(), "possible caller is", "possible callers are")
                ));
            }
            Ok(out)
        })())
    }

    #[tool(
        description = "The functions of this checkout that a symbol calls, and the names outside the checkout that it calls. The result has the certain calls only, unless `possible` is true. A possible call has one line for each function with that name, and the result marks it."
    )]
    async fn callees(
        &self,
        Parameters(params): Parameters<EdgeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let index = self.open()?;
            let s = find(&index, &params.symbol, params.file.as_deref())?;
            let possible = params.possible.unwrap_or(false);
            let (edges, externals) = index.callees(s.id, possible)?;
            let mut out = format!(
                "{} calls {} in the checkout:",
                described(&s),
                plural(edges.len(), "function", "functions")
            );
            for e in &edges {
                out.push_str(&format!(
                    "\n- {}, call at line {}, {}",
                    described(&e.other),
                    e.line,
                    marks(e)
                ));
            }
            if !externals.is_empty() {
                let mut seen = std::collections::HashSet::new();
                let names: Vec<&str> = externals
                    .iter()
                    .map(|e| e.name.as_str())
                    .filter(|name| seen.insert(*name))
                    .collect();
                out.push_str(&format!("\nOutside the checkout: {}", names.join(", ")));
            }
            if !possible {
                let all = index.callees(s.id, true)?.0.len();
                if all > edges.len() {
                    out.push_str(&format!(
                        "\n{} not shown. Set `possible` to true to see them.",
                        plural(
                            all - edges.len(),
                            "possible callee is",
                            "possible callees are"
                        )
                    ));
                }
            }
            Ok(out)
        })())
    }

    #[tool(
        description = "How one symbol of this checkout reaches another: the shortest chain of certain calls from `from` to `to`, one call on each line, in order. Possible calls are not followed."
    )]
    async fn path(
        &self,
        Parameters(params): Parameters<PathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let index = self.open()?;
            let from = find(&index, &params.from, params.from_file.as_deref())?;
            let to = find(&index, &params.to, params.to_file.as_deref())?;
            let Some(steps) = index.path(from.id, to.id)? else {
                return Ok(format!(
                    "No chain of certain calls goes from {} to {}.",
                    described(&from),
                    described(&to)
                ));
            };
            let mut out = format!(
                "{} from {} to {}:",
                plural(steps.len(), "call", "calls"),
                described(&from),
                described(&to)
            );
            for (i, step) in steps.iter().enumerate() {
                out.push_str(&format!(
                    "\n{}. {} calls {} ({}), at {}:{}",
                    i + 1,
                    step.caller.qualified(),
                    step.callee.qualified(),
                    step.callee.place(),
                    step.caller.file,
                    step.line
                ));
                if step.scip_pending {
                    out.push_str(", by name until the next SCIP run");
                }
            }
            Ok(out)
        })())
    }

    #[tool(
        description = "Find repeated code in this checkout. The kinds: `exact`, the same code with other whitespace or comments; `near`, a copy with renamed names or a few changed lines; `same-idea`, 2 functions whose summaries have the same meaning. Each group has its kind, a score from 0 to 1, and its functions with their files and lines. Test code and functions under 5 lines are left out, unless you set `tests` or `min_lines`."
    )]
    async fn duplicates(
        &self,
        Parameters(params): Parameters<DuplicatesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let limit = params.limit.unwrap_or(DEFAULT_GROUPS);
            if !(1..=MAX_GROUPS).contains(&limit) {
                return Err(ToolError::Limit(MAX_GROUPS));
            }
            let min_lines = params.min_lines.unwrap_or(DEFAULT_MIN_LINES);
            if min_lines == 0 {
                return Err(ToolError::MinLines);
            }
            let options = DuplicateOptions {
                kinds: match params.kind {
                    Some(kind) => vec![kind.into()],
                    None => DuplicateKind::ALL.to_vec(),
                },
                min_lines,
                include_tests: params.tests.unwrap_or(false),
                under: params.under,
                ..DuplicateOptions::default()
            };
            let index = self.open()?;
            Ok(duplicates_text(&index, &options, limit)?)
        })())
    }

    #[tool(
        description = "The map of this checkout: each folder of source files, with its summary and its files, each file with its summary. Use it in place of .metis/code-index.md. `under` keeps the folders under one path."
    )]
    async fn module_map(
        &self,
        Parameters(params): Parameters<ModuleMapParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let index = self.open()?;
            let modules = index.module_map(params.under.as_deref())?;
            let files: usize = modules.iter().map(|m| m.files.len()).sum();
            let mut out = format!(
                "{}, {}.",
                plural(modules.len(), "module", "modules"),
                plural(files, "file", "files")
            );
            if modules
                .iter()
                .all(|m| m.summary.is_none() && m.files.iter().all(|f| f.summary.is_none()))
            {
                out.push_str(" The index has no summaries.");
            }
            for m in &modules {
                out.push_str(&format!("\n\n## {}\n", m.path));
                out.push_str(m.summary.as_deref().unwrap_or("No summary."));
                for f in &m.files {
                    match &f.summary {
                        Some(summary) => out.push_str(&format!("\n- {}: {summary}", f.path)),
                        None => out.push_str(&format!(
                            "\n- {} ({})",
                            f.path,
                            plural(f.symbols, "symbol", "symbols")
                        )),
                    }
                }
            }
            Ok(out)
        })())
    }
}

// `call_tool` is by hand: the refusal of the arguments comes from the
// extractor as an error, and the agent gets it as a tool error.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for CodeTools {
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = ToolCallContext::new(self, request, context);
        match self.tool_router.call(call).await {
            Ok(result) => Ok(result),
            Err(e) => tool_error_of(e),
        }
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("kairos-code", env!("CARGO_PKG_VERSION"))
                    .with_title("Kairos code index"),
            )
            .with_instructions(format!(
                "The code index of the checkout {}. Use these tools before you read files: \
                 `module_map` for the map, `code_search` to find code by what it does, \
                 `symbol` for one symbol, `callers` and `callees` for the call graph, \
                 `path` for how one symbol reaches another, and `duplicates` for repeated \
                 code. Run `kairos index update` after \
                 you change code.",
                display(&self.root)
            ))
    }
}

fn display(path: &Path) -> String {
    path.display().to_string()
}
