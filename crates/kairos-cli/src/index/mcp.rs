//! The local MCP server of the code tools (COLLIERY-I-0264, "What an agent
//! gets"; COLLIERY-T-1852): `kairos index mcp` talks MCP over stdio and
//! reads the index of one checkout.
//!
//! Each tool call opens the index again. So a `kairos index update` that
//! runs while the server runs is seen at the next call. The results are
//! short texts (ASD-STE100), as the results of the Kairos server are.
//!
//! The checkout that the tools read is the one the server started in, when
//! that is a git checkout. The agent can open another one, or the first one
//! when the session started outside a checkout, with `fetch_index`: the
//! server takes the nearest index of the repository from Kairos and updates
//! it for the files that differ (KAIROS-T-0348).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kairos_embed::EmbeddingProvider;
use kairos_index::{
    CallEdge, Coverage, DEFAULT_MIN_LINES, DuplicateKind, DuplicateOptions, FileInfo, Index,
    IndexError, Lookup, Reference, SearchHit, SearchMode, SymbolInfo, SymbolSource, UpdateOptions,
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
use super::error::{
    Below, Choose, IndexCommandError, Place, StillOpen, SymbolArguments, ToolError,
};
use super::query::{QuerySource, ROUTE_TIMEOUT, RouteFailure};

/// The results of `code_search` when the call gives no `limit`.
const DEFAULT_LIMIT: usize = 10;
/// The most results of one `code_search`.
const MAX_LIMIT: usize = 50;
/// The most bytes of source text that `symbol` gives (KAIROS-T-0420).
const SOURCE_LIMIT: usize = 16 * 1024;
/// The uses of `references` when the call gives no `limit` (KAIROS-T-0419).
pub(super) const DEFAULT_USES: usize = 100;
/// The most uses of one `references`.
pub(super) const MAX_USES: usize = 1000;
/// The groups of `duplicates` when the call gives no `limit`.
pub(super) const DEFAULT_GROUPS: usize = 20;
/// The most groups of one `duplicates`.
pub(super) const MAX_GROUPS: usize = 200;

/// Serve the code tools on stdin and stdout until the client closes stdin.
/// The tools read the checkout at `root` when it is one; otherwise the
/// agent opens a checkout with `fetch_index`.
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

/// The source of the query vectors, chosen on the first search that needs
/// it: the checkout, the model of its index, and the source
/// (KAIROS-T-0360).
type QueryEmbedder = (Option<PathBuf>, String, QuerySource);

/// The most checkouts below the start folder that the server names (#272),
/// as the session hook of the plugin lists.
const MAX_BELOW: usize = 40;

/// A checkout below the start folder of the server, when that folder is
/// not a checkout (#272): a board folder, or a folder of work that holds
/// clones and worktrees of more than one repository.
#[derive(Debug, Clone, PartialEq)]
struct Found {
    name: String,
    path: PathBuf,
    /// The `origin` remote as `host/path`, or `None` with no `origin`.
    remote: Option<String>,
    /// The checkout has a local index.
    indexed: bool,
}

impl Found {
    /// `engine (/work/engine, origin github.com/acme/engine, a local index)`.
    fn line(&self) -> String {
        let remote = match &self.remote {
            Some(remote) => format!("origin {remote}"),
            None => "no origin remote".to_string(),
        };
        let index = if self.indexed {
            "a local index"
        } else {
            "no local index"
        };
        format!("{} ({}, {remote}, {index})", self.name, display(&self.path))
    }
}

/// True when the checkout at `root` has a local index: in the user cache
/// (KAIROS-A-0072), or an old one in the checkout that the next open moves.
fn has_index(root: &Path) -> bool {
    super::db_path(root).is_file() || root.join(kairos_index::INDEX_FILE).is_file()
}

/// The checkouts in the immediate subfolders of `folder`, as `kairos repos
/// local` finds them, at most [`MAX_BELOW`]. No git command fails the
/// start of the server: a folder that cannot be read gives none.
fn checkouts_below(folder: &Path) -> Vec<Found> {
    let all = crate::commands::repos_local::checkouts_in(folder).unwrap_or_default();
    all.into_iter()
        .take(MAX_BELOW)
        .map(|(name, path)| Found {
            remote: super::git_output(&path, &["config", "--get", "remote.origin.url"])
                .map(|url| super::base::normalized_remote(&url)),
            indexed: has_index(&path),
            name,
            path,
        })
        .collect()
}

#[derive(Clone)]
pub struct CodeTools {
    /// The open checkouts (#272): the one the server started in, then each
    /// one that `fetch_index` opened, oldest first. The last one is the
    /// default of the tools. Empty until a checkout is open.
    open: Arc<Mutex<Vec<PathBuf>>>,
    /// The checkouts below the start folder when it is not a checkout
    /// (#272). The instructions and the error "no open checkout" name them.
    below: Arc<Vec<Found>>,
    /// The source of the query vectors, one for each checkout and model.
    embedder: Arc<Mutex<Vec<QueryEmbedder>>>,
    tool_router: ToolRouter<Self>,
}

// The arguments of each tool. `deny_unknown_fields` is the rule of the
// Kairos MCP tools: an argument that does nothing tells the agent that it
// did something.

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct FetchIndexParams {
    /// The path of the checkout: any folder inside a git checkout.
    pub root: String,
    /// The repository in Kairos, by its slug. Default: the repository whose URL is the `origin` remote of the checkout.
    pub repository: Option<String>,
    /// The most files that can change since the index that the server starts from. Above it, the server does not change the index. Default: no limit, because `fetch_index` runs no model.
    pub max_changed: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    /// What the code does, in plain words.
    pub query: String,
    /// The most results to give, from 1 to 50. Default 10.
    pub limit: Option<usize>,
    /// The open checkout to search: its folder name, or its path. Default:
    /// each open checkout.
    pub checkout: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SymbolParams {
    /// A name, `Type::name` (Rust) or `Type.name`. A candidate from the list
    /// of an AMBIGUOUS error, `name (file:line)`, is also correct.
    pub symbol: String,
    /// The file of the symbol, from the root of the checkout, or the end of
    /// its path. Give it when more than one symbol has the name.
    pub file: Option<String>,
    /// A line of the symbol: the symbol that starts on it, or else the
    /// smallest symbol that holds it. Give it when more than one symbol of
    /// one file has the name.
    pub line: Option<u32>,
    /// Also give the source text of the symbol, from the file in the
    /// checkout, at most 16384 bytes. Default false.
    pub source: Option<bool>,
    /// The open checkout to read: its folder name, or its path. Default: the
    /// last checkout that `fetch_index` opened.
    pub checkout: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct EdgeParams {
    /// A name, `Type::name` (Rust) or `Type.name`. A candidate from the list
    /// of an AMBIGUOUS error, `name (file:line)`, is also correct.
    pub symbol: String,
    /// The file of the symbol, when more than one symbol has the name.
    pub file: Option<String>,
    /// A line of the symbol, when more than one symbol of one file has the
    /// name: the symbol that starts on it, or else the smallest symbol that
    /// holds it.
    pub line: Option<u32>,
    /// Also give the possible calls: calls by a name that more than one
    /// function has. Default false.
    pub possible: Option<bool>,
    /// The open checkout to read: its folder name, or its path. Default: the
    /// last checkout that `fetch_index` opened.
    pub checkout: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ReferencesParams {
    /// A name, `Type::name` (Rust) or `Type.name`. A candidate from the list
    /// of an AMBIGUOUS error, `name (file:line)`, is also correct. An ADR
    /// short code (`KAIROS-A-0065`) gives each place that cites the ADR.
    pub symbol: String,
    /// The file of the symbol, when more than one symbol has the name.
    pub file: Option<String>,
    /// A line of the symbol, when more than one symbol of one file has the
    /// name: the symbol that starts on it, or else the smallest symbol that
    /// holds it.
    pub line: Option<u32>,
    /// Also give the possible calls: calls by a name that more than one
    /// function has. Default false.
    pub possible: Option<bool>,
    /// The most uses to give, from 1 to 1000. Default 100.
    pub limit: Option<usize>,
    /// The open checkout to read: its folder name, or its path. Default: the
    /// last checkout that `fetch_index` opened.
    pub checkout: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct PathParams {
    /// The symbol where the chain starts, as `symbol` of the tool `symbol`.
    pub from: String,
    /// The symbol where the chain stops, as `symbol` of the tool `symbol`.
    pub to: String,
    /// The file of `from`, when more than one symbol has the name.
    pub from_file: Option<String>,
    /// The file of `to`, when more than one symbol has the name.
    pub to_file: Option<String>,
    /// A line of `from`, when more than one symbol of one file has the name.
    pub from_line: Option<u32>,
    /// A line of `to`, when more than one symbol of one file has the name.
    pub to_line: Option<u32>,
    /// The open checkout to read: its folder name, or its path. Default: the
    /// last checkout that `fetch_index` opened.
    pub checkout: Option<String>,
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
    /// The open checkout to read: its folder name, or its path. Default: the
    /// last checkout that `fetch_index` opened.
    pub checkout: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ModuleMapParams {
    /// Only the folders under this path, from the root of the checkout.
    pub under: Option<String>,
    /// The open checkout to read: its folder name, or its path. Default: the
    /// last checkout that `fetch_index` opened.
    pub checkout: Option<String>,
}

impl CodeTools {
    /// The tools of `root`. A folder that is not a git checkout and has no
    /// index opens nothing: the agent opens a checkout with `fetch_index`,
    /// and the server names the checkouts below the folder (#272).
    pub fn new(root: PathBuf) -> Self {
        let open = has_index(&root) || root.join(".git").exists();
        let below = if open {
            Vec::new()
        } else {
            checkouts_below(&root)
        };
        CodeTools {
            open: Arc::new(Mutex::new(if open { vec![root] } else { Vec::new() })),
            below: Arc::new(below),
            embedder: Arc::new(Mutex::new(Vec::new())),
            tool_router: Self::tool_router(),
        }
    }

    /// The default checkout of the tools: the last one opened, or `None`.
    fn root(&self) -> Option<PathBuf> {
        self.opened().pop()
    }

    /// The open checkouts, oldest first.
    fn opened(&self) -> Vec<PathBuf> {
        self.open
            .lock()
            .expect("the lock of the open checkouts")
            .clone()
    }

    /// Open `root`, or make it the default again when it is open already.
    fn add_open(&self, root: PathBuf) {
        let mut open = self.open.lock().expect("the lock of the open checkouts");
        open.retain(|p| *p != root);
        open.push(root);
    }

    /// The error for a call with no open checkout: it names the checkouts
    /// below the start folder.
    fn no_checkout(&self) -> ToolError {
        ToolError::NoCheckout(Below(self.below.iter().map(Found::line).collect()))
    }

    /// The checkout that a call reads (#272): the one that `checkout` names,
    /// by its folder name or its path, or else the default.
    fn pick(&self, checkout: Option<&str>) -> Result<PathBuf, ToolError> {
        let open = self.opened();
        let Some(given) = checkout.map(str::trim).filter(|c| !c.is_empty()) else {
            return open.last().cloned().ok_or_else(|| self.no_checkout());
        };
        if open.is_empty() {
            return Err(self.no_checkout());
        }
        let as_path = Path::new(given).canonicalize().ok();
        let matches: Vec<&PathBuf> = open
            .iter()
            .filter(|p| {
                as_path.as_ref() == Some(*p) || p.file_name().is_some_and(|name| name == given)
            })
            .collect();
        match matches.as_slice() {
            [one] => Ok((*one).clone()),
            many => Err(ToolError::UnknownCheckout {
                given: given.to_string(),
                more_than_one: !many.is_empty(),
                open: open.iter().map(|p| display(p)).collect(),
            }),
        }
    }

    /// The index of the checkout that a call reads.
    fn open_index(&self, checkout: Option<&str>) -> Result<(PathBuf, Index), ToolError> {
        let root = self.pick(checkout)?;
        let index = Self::index_at(&root)?;
        Ok((root, index))
    }

    fn index_at(root: &Path) -> Result<Index, ToolError> {
        let (db, _moved) = super::prepare_quiet(root)?;
        if !db.is_file() {
            return Err(ToolError::NoIndex(db));
        }
        Ok(Index::open(&db)?)
    }

    /// `fetch_index`: open the checkout at `params.root`. Kairos gives the
    /// nearest index of the repository; the server puts it at the index of
    /// the checkout in the user cache (KAIROS-A-0072), updates the structure for the
    /// files that differ, and links the summaries of the pool. It runs no
    /// model. With no base from Kairos, a local index is used when there is
    /// one.
    async fn fetch(&self, params: FetchIndexParams) -> Result<String, ToolError> {
        let given = PathBuf::from(&params.root);
        let folder = given
            .canonicalize()
            .ok()
            .filter(|p| p.is_dir())
            .ok_or_else(|| ToolError::NotACheckout(given.clone()))?;
        let root = super::git_output(&folder, &["rev-parse", "--show-toplevel"])
            .map(PathBuf::from)
            .ok_or_else(|| ToolError::NotACheckout(given.clone()))?;
        let root = root.canonicalize().unwrap_or(root);

        let remote = super::base::RemoteArgs {
            repository: params.repository,
            ..Default::default()
        };
        let found = super::base::find(&root, &remote).await;
        // KAIROS-T-0515: this path runs no model, so it has no limit unless
        // the agent gives one.
        let limit = super::limit_of(params.max_changed, false);
        // KAIROS-T-0526: this server is the `kairos` binary of the session
        // start. Say so when the deployment is newer.
        let behind = crate::self_update::behind_server_line().await;
        let (db, _moved) = super::prepare_quiet(&root)?;
        // One process at a time (KAIROS-T-0357): another agent can fetch the
        // index of this checkout now. The wait is off the async threads.
        let _lock = {
            let db = db.clone();
            tokio::task::spawn_blocking(move || super::lock_index(&db))
                .await
                .map_err(|e| IndexCommandError::Server(e.to_string()))??
        };
        let had_index = db.is_file();
        let mut lines = Vec::new();
        let mut hosted = false;
        // The count of the files of the tree that differ from the local
        // index, when the CLI can read it.
        let local_changed = if had_index {
            let (root, db) = (root.clone(), db.clone());
            tokio::task::spawn_blocking(move || {
                kairos_index::changed_files(&root, &db)
                    .ok()
                    .map(|c| c.len())
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        match found {
            // The local index is nearer to the tree than the base: it keeps
            // its structure, and takes the summaries of the base when the
            // base has more (KAIROS-T-0355), as `kairos index update` does.
            super::base::Found::Base(b) if local_changed.is_some_and(|n| n <= b.changed) => {
                let n = local_changed.unwrap_or_default();
                if let Some(limit) = limit
                    && n > limit
                {
                    return Err(super::too_far(
                        &root,
                        &b,
                        n,
                        limit,
                        true,
                        super::error::Caller::Tool,
                    )
                    .into());
                }
                lines.push(format!(
                    "Repository {}: the local index is nearer to the tree than the index of \
                     commit {} from Kairos ({} changed, against {}). The local index is used.",
                    b.repository,
                    b.short(),
                    plural(n, "file", "files"),
                    b.changed
                ));
                if let Some(line) = super::take_base_summaries(&db, &b).await {
                    lines.push(line);
                }
                hosted = b.hosted;
            }
            super::base::Found::Base(b) => {
                if let Some(limit) = limit
                    && b.changed > limit
                {
                    return Err(super::too_far(
                        &root,
                        &b,
                        b.changed,
                        limit,
                        false,
                        super::error::Caller::Tool,
                    )
                    .into());
                }
                let bytes = b.download().await.map_err(|why| ToolError::Download {
                    commit: b.short().to_string(),
                    why,
                })?;
                if let Some(warning) = super::install_quiet(&db, &bytes, had_index)? {
                    lines.push(warning);
                }
                lines.push(format!(
                    "Repository {}: the index of commit {} from Kairos, {} changed since it.",
                    b.repository,
                    b.short(),
                    plural(b.changed, "file", "files")
                ));
                hosted = b.hosted;
            }
            found => {
                let why = super::reason(&found);
                if !had_index {
                    return Err(ToolError::NoBaseIndex { root, why });
                }
                lines.push(format!(
                    "Kairos gave no base index, because {why}. The local index is used."
                ));
            }
        }

        // The update, off the async threads: it parses the changed files.
        let (report, outcome) = {
            let root = root.clone();
            let db = db.clone();
            tokio::task::spawn_blocking(move || -> Result<_, IndexCommandError> {
                let options = UpdateOptions {
                    rust_edges: false,
                    ..UpdateOptions::default()
                };
                let report = kairos_index::update_structure(&root, &db, &options)?;
                let no_model = if hosted {
                    super::summaries::HOSTED
                } else {
                    super::summaries::LINK_ONLY
                };
                let outcome = super::summaries::run(&root, &db, Some(no_model))?;
                Ok((report, outcome))
            })
            .await
            .map_err(|e| IndexCommandError::Server(e.to_string()))??
        };
        let summary = match &outcome {
            super::summaries::Outcome::Linked(r) | super::summaries::Outcome::NotMade(r, _) => r,
            #[cfg(feature = "llama")]
            super::summaries::Outcome::Made(r, _) => r,
        };
        let linked = summary.symbols.reused + summary.files.reused + summary.modules.reused;
        let left = summary.symbols.left + summary.files.left + summary.modules.left;

        self.add_open(root.clone());
        lines.insert(
            0,
            format!("The server opened the index of {}.", display(&root)),
        );
        lines.push(format!(
            "{}, {}, {} linked from the pool, {} with none. The tools answer for this checkout now.",
            plural(report.files, "file", "files"),
            plural(report.symbols, "symbol", "symbols"),
            plural(linked, "summary", "summaries"),
            left
        ));
        if let Some(line) = open_line(&self.opened()) {
            lines.push(line);
        }
        if let Some(line) = behind {
            lines.push(line);
        }
        Ok(lines.join(" "))
    }

    /// The source of the query vectors for the model of the pool, kept for
    /// the checkout and the model.
    fn query_source(&self, root: &Path, model: &str) -> QuerySource {
        let mut cached = self.embedder.lock().expect("the lock of the embedder");
        if let Some((_, _, source)) = cached
            .iter()
            .find(|(r, m, _)| r.as_deref() == Some(root) && m == model)
        {
            return source.clone();
        }
        let source = super::query::query_source(model, Some(root));
        cached.push((Some(root.to_path_buf()), model.to_string(), source.clone()));
        source
    }

    /// The provider of the vector of `query`, and whether Kairos made the
    /// vector; or why there is none. A lasting refusal of Kairos is kept as
    /// the reason, so the next search does not ask again.
    async fn query_provider(
        &self,
        root: &Path,
        model: &str,
        query: &str,
    ) -> Result<(Arc<dyn EmbeddingProvider>, bool), String> {
        let source = self.query_source(root, model);
        let from_kairos = matches!(source, QuerySource::Kairos(_));
        match super::query::query_provider(&source, model, query, ROUTE_TIMEOUT).await {
            Ok(provider) => Ok((provider, from_kairos)),
            Err(RouteFailure::Once(why)) => Err(why),
            Err(RouteFailure::Lasting(why)) => {
                if from_kairos {
                    let mut cached = self.embedder.lock().expect("the lock of the embedder");
                    if let Some((_, _, source)) = cached
                        .iter_mut()
                        .find(|(r, m, _)| r.as_deref() == Some(root) && m == model)
                    {
                        *source = QuerySource::Text(why.clone());
                    }
                }
                Err(why)
            }
        }
    }

    /// One search of one checkout: its hits, and the line that says how the
    /// search ran.
    async fn search_one(
        &self,
        root: &Path,
        query: &str,
        limit: usize,
    ) -> Result<(Vec<SearchHit>, SearchMode, String), ToolError> {
        // The vector of the query first: Kairos can make it (KAIROS-T-0360),
        // and the index is not held over the request.
        let mut why_no_vectors = None;
        let mut from_kairos = false;
        let embedder = match Self::index_at(root)?.vector_model()? {
            None => None,
            Some(model) => match self.query_provider(root, &model, query).await {
                Ok((provider, kairos)) => {
                    from_kairos = kairos;
                    Some(provider)
                }
                Err(why) => {
                    why_no_vectors = Some(why);
                    None
                }
            },
        };
        let found = Self::index_at(root)?.search(query, embedder.as_deref(), limit)?;
        let how = match found.mode {
            SearchMode::Vectors if from_kairos => "The search used the summaries and their \
                 vectors. Kairos made the vector of the query."
                .to_string(),
            SearchMode::Vectors => "The search used the summaries and their vectors.".to_string(),
            SearchMode::SummaryText => format!(
                "The search used the text of the summaries only. {}",
                why_no_vectors.unwrap_or_else(|| "The summaries have no vectors.".into())
            ),
            SearchMode::Names => "The index has no summaries. So the search used the names, \
                 the signatures and the paths only. A build with the summarizer adds them."
                .to_string(),
        };
        Ok((found.hits, found.mode, how))
    }
}

/// The line of `fetch_index` with more than one open checkout (#272): the
/// tools read the default, and `checkout` chooses another one.
fn open_line(open: &[PathBuf]) -> Option<String> {
    (open.len() > 1).then(|| {
        let names: Vec<String> = open.iter().map(|p| checkout_name(p)).collect();
        format!(
            "{} are open: {}. Give `checkout` to read another one. `code_search` with no \
             `checkout` searches each of them.",
            plural(open.len(), "checkout", "checkouts"),
            names.join(", ")
        )
    })
}

/// The folder name of a checkout, as `checkout` takes it.
fn checkout_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| display(root))
}

/// The arguments that choose a symbol in `symbol`, `callers`, `callees`
/// and `references`.
pub(super) const SYMBOL: SymbolArguments = SymbolArguments {
    symbol: "symbol",
    file: "file",
    line: "line",
};
/// The arguments of `path` for `from`.
const FROM: SymbolArguments = SymbolArguments {
    symbol: "from",
    file: "from_file",
    line: "from_line",
};
/// The arguments of `path` for `to`.
const TO: SymbolArguments = SymbolArguments {
    symbol: "to",
    file: "to_file",
    line: "to_line",
};

/// Find the symbol, or the error that says why not. Each tool that takes a
/// symbol finds it here (KAIROS-T-0417).
pub(super) fn find(
    index: &Index,
    symbol: &str,
    file: Option<&str>,
    line: Option<u32>,
    args: SymbolArguments,
) -> Result<SymbolInfo, ToolError> {
    match index.lookup(symbol, file, line)? {
        Lookup::Found(found) => Ok(found),
        Lookup::Missing => Err(ToolError::NoSymbol {
            name: symbol.trim().to_string(),
            place: Place::of(file, line),
        }),
        Lookup::Ambiguous(all) => Err(ambiguous(symbol, &all, args)),
    }
}

/// The error for more than one symbol: the candidates, and the argument
/// that chooses one of them.
fn ambiguous(symbol: &str, all: &[SymbolInfo], args: SymbolArguments) -> ToolError {
    let files: std::collections::HashSet<&str> = all.iter().map(|s| s.file.as_str()).collect();
    ToolError::Ambiguous {
        name: symbol.trim().to_string(),
        count: all.len(),
        candidates: all.iter().take(20).map(described).collect(),
        choose: Choose::of(files.len(), all.len(), args),
    }
}

/// The source part of the result of `symbol` (KAIROS-T-0420): a line that
/// says what the text is, then the text in a code fence.
fn source_text(s: &SymbolInfo, source: Option<SymbolSource>) -> String {
    let Some(source) = source else {
        return format!(
            "\nSource: none. The file {} is not in the checkout now.",
            s.file
        );
    };
    let mut out = format!(
        "\nSource: lines {}-{}, {}.",
        s.start_line,
        s.end_line,
        plural(source.bytes, "byte", "bytes")
    );
    if source.changed {
        out.push_str(
            " Warning: the file changed after the last index update. The source can be \
             different from the symbol. Run `kairos index update`.",
        );
    }
    if source.cut {
        let next = s.start_line as usize + source.text.matches('\n').count();
        out.push_str(&format!(
            " The text is cut at the limit of {SOURCE_LIMIT} bytes: it has {} of {}. \
             Read {} from line {next} for the rest.",
            source.text.len(),
            source.bytes,
            s.file
        ));
    }
    // A fence that is longer than each run of backticks in the text.
    let longest = source
        .text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    let text = source.text.trim_end_matches('\n');
    out.push_str(&format!("\n{fence}{}\n{text}\n{fence}", s.language));
    out
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

/// The text of a failed `fetch_index`: the error, then the checkout that the
/// tools still answer for (KAIROS-T-0517).
fn fetch_error(e: &ToolError, previous: Option<&Path>) -> String {
    let still = match previous {
        Some(root) => StillOpen::Root(display(root)),
        None => StillOpen::Nothing,
    };
    e.text_with(Some(&still))
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

/// The marks of a use after its kind (KAIROS-T-0419): how the index knows
/// a call that SCIP did not give.
fn use_marks(r: &Reference) -> String {
    let mut out = String::new();
    if r.origin != "scip" {
        out.push_str(", by name");
    }
    if r.class == "possible" {
        out.push_str(&format!(
            ", possible: 1 of {} functions with this name",
            r.candidates
        ));
    }
    if r.origin == "macro-text" {
        out.push_str(", in the text of a macro");
    }
    if r.scip_pending {
        out.push_str(", until the next SCIP run");
    }
    out
}

/// The text of `references`, for the MCP tool and for `kairos index
/// references` (KAIROS-T-0419): the count of the uses by kind, what the
/// index has for the language, then the uses by file.
pub(super) fn references_text(
    index: &Index,
    s: &SymbolInfo,
    possible: bool,
    limit: usize,
) -> Result<String, IndexError> {
    let found = index.references(s.id, possible)?;
    let uses = &found.uses;
    let files: std::collections::BTreeSet<&str> = uses.iter().map(|u| u.file.as_str()).collect();
    let mut out = format!("{} of {}", plural(uses.len(), "use", "uses"), described(s));
    if uses.is_empty() {
        out.push('.');
    } else {
        let kinds: Vec<String> = ["call", "type", "field", "import", "other"]
            .iter()
            .filter_map(|kind| {
                let n = uses.iter().filter(|u| u.kind == *kind).count();
                (n > 0).then(|| format!("{n} {kind}"))
            })
            .collect();
        out.push_str(&format!(
            " in {}: {}.",
            plural(files.len(), "file", "files"),
            kinds.join(", ")
        ));
    }
    match found.coverage {
        Coverage::Full => {}
        Coverage::Pending(n) if s.language == "rust" => out.push_str(&format!(
            " {} after the last SCIP run: in them, the result has the calls only. Run \
             `kairos index update --rust-edges` to get all the uses.",
            plural(n, "Rust file changed", "Rust files changed")
        )),
        Coverage::Pending(n) => out.push_str(&format!(
            " {} after the last SCIP run: in them, the result has the calls only. Run \
             `kairos index update --scip` to get all the uses.",
            plural(
                n,
                &format!("{} file changed", s.language),
                &format!("{} files changed", s.language)
            )
        )),
        Coverage::CallsOnly if s.language == "rust" => out.push_str(
            " The index has no SCIP data for the Rust files, so the result has the calls only. \
             A build in a Cargo workspace runs SCIP.",
        ),
        Coverage::CallsOnly if kairos_index::SCIP_LANGUAGES.contains(&s.language.as_str()) => out
            .push_str(&format!(
                " For {0} code, the index has the calls only: the uses as a type, a field or an \
                 import are not in the result. A build runs SCIP for {0} code when the SCIP \
                 indexer of the language is there (see the warnings of the build).",
                s.language
            )),
        Coverage::CallsOnly => out.push_str(&format!(
            " For {} code, the index has the calls only: the uses as a type, a field or an \
             import are not in the result.",
            s.language
        )),
    }
    if uses.len() > limit {
        out.push_str(&format!(
            " Only the first {limit} are shown. Give a larger `limit`, up to {MAX_USES}."
        ));
    }
    let mut file = "";
    for u in uses.iter().take(limit) {
        if u.file != file {
            file = &u.file;
            out.push_str(&format!("\n{file}:"));
        }
        let place = match &u.enclosing {
            Some(e) => format!("in {}", e.qualified()),
            None => "at the top of the file".to_string(),
        };
        out.push_str(&format!(
            "\n- line {}: {}, {place}{}",
            u.line,
            u.kind,
            use_marks(u)
        ));
    }
    if !possible {
        let all = index.references(s.id, true)?.uses.len();
        if all > uses.len() {
            out.push_str(&format!(
                "\n{} not shown. Set `possible` to true to see them.",
                plural(all - uses.len(), "possible call is", "possible calls are")
            ));
        }
    }
    Ok(out)
}

/// The text of `references` for an ADR short code, for the MCP tool and
/// for `kairos index references` (KAIROS-T-0703): the count of the places
/// by context, then the places by file, each with its symbol.
pub(super) fn citations_text(
    index: &Index,
    code: &str,
    limit: usize,
) -> Result<String, IndexError> {
    let places = index.adr_citations(code)?;
    if places.is_empty() {
        return Ok(format!(
            "No place in the code of this checkout cites {code}. The index reads the ADR short \
             codes in comments, in doc strings and in the names of tests."
        ));
    }
    let files: std::collections::BTreeSet<&str> = places.iter().map(|p| p.file.as_str()).collect();
    let contexts: Vec<String> = ["doc", "comment", "test"]
        .iter()
        .filter_map(|context| {
            let n = places.iter().filter(|p| p.context == *context).count();
            (n > 0).then(|| format!("{n} {context}"))
        })
        .collect();
    let mut out = format!(
        "{} {code} in {}: {}.",
        plural(places.len(), "place cites", "places cite"),
        plural(files.len(), "file", "files"),
        contexts.join(", ")
    );
    if places.len() > limit {
        out.push_str(&format!(
            " Only the first {limit} are shown. Give a larger `limit`, up to {MAX_USES}."
        ));
    }
    let mut file = "";
    for p in places.iter().take(limit) {
        if p.file != file {
            file = &p.file;
            out.push_str(&format!("\n{file}:"));
        }
        let place = match &p.symbol {
            Some(s) => format!("about {}", s.qualified()),
            None => "at the top of the file".to_string(),
        };
        out.push_str(&format!("\n- line {}: {}, {place}", p.line, p.context));
    }
    Ok(out)
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

pub(super) fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

#[tool_router]
impl CodeTools {
    #[tool(
        description = "Open the index of a checkout. Give `root`, the path of the checkout. The server takes the nearest index of its repository from Kairos, updates it for the files that differ, and the other tools answer for this checkout by default. The checkouts opened before stay open: give `checkout` to a tool to read one of them. Give `repository`, the slug in Kairos, when the origin remote of the checkout is not a repository of Kairos. It runs no model, so it has no limit of changed files. Give `max_changed` for a limit. A call that fails does not change the open checkout. Call it first when the session did not start inside a checkout."
    )]
    async fn fetch_index(
        &self,
        Parameters(params): Parameters<FetchIndexParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // KAIROS-T-0517: a failed fetch does not change the open checkout.
        // The error says which one the tools still answer for.
        let previous = self.root();
        Ok(match self.fetch(params).await {
            Ok(t) => text(t),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(fetch_error(
                &e,
                previous.as_deref(),
            ))]),
        })
    }

    #[tool(
        description = "Search the code by what it does. Give a description in plain words. The result is the best functions and types, with their files, lines and summaries. With more than one open checkout, the search reads each of them and each result names its checkout; give `checkout` to search one only. If an index has no summaries, the search reads the names, the signatures and the paths, and the result says so. Test code ranks below the other code. To find tests, put a word such as \"test\" or \"tested\" in the description."
    )]
    async fn code_search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return result(Err(ToolError::Limit(MAX_LIMIT)));
        }
        // One checkout when the call names it; else each open one (#272).
        let roots = match params.checkout.as_deref() {
            Some(checkout) => match self.pick(Some(checkout)) {
                Ok(root) => vec![root],
                Err(e) => return result(Err(e)),
            },
            None => self.opened(),
        };
        if roots.is_empty() {
            return result(Err(self.no_checkout()));
        }
        let many = roots.len() > 1;
        let mut hows = Vec::new();
        let mut hits: Vec<(String, SearchMode, SearchHit)> = Vec::new();
        for root in &roots {
            let name = checkout_name(root);
            match self.search_one(root, &params.query, limit).await {
                Ok((found, mode, how)) => {
                    hows.push(if many { format!("{name}: {how}") } else { how });
                    hits.extend(found.into_iter().map(|hit| (name.clone(), mode, hit)));
                }
                // One checkout that cannot be read does not stop the others.
                Err(e) if many => hows.push(format!("{name}: no search. {}", e.text())),
                Err(e) => return result(Err(e)),
            }
        }
        // The best hits of all the checkouts. The scores of two checkouts
        // compare only when both searches used the same mode.
        hits.sort_by(|a, b| b.2.score.total_cmp(&a.2.score));
        hits.truncate(limit);
        let mut out = hows.join("\n");
        if hits.is_empty() {
            out.push_str(&format!("\nNo result for {:?}.", params.query));
            return result(Ok(out));
        }
        out.push_str(&format!(
            "\n{} for {:?}:",
            plural(hits.len(), "result", "results"),
            params.query
        ));
        for (i, (name, mode, hit)) in hits.iter().enumerate() {
            let s = &hit.symbol;
            let checkout = if many {
                format!("[{name}] ")
            } else {
                String::new()
            };
            out.push_str(&format!(
                "\n{}. {checkout}{}, {}",
                i + 1,
                described(s),
                s.kind
            ));
            if s.is_test {
                out.push_str(", test code");
            }
            match (&s.summary, &s.signature) {
                (Some(summary), _) => out.push_str(&format!(": {summary}")),
                (None, Some(signature)) => out.push_str(&format!(": `{signature}`")),
                (None, None) => {}
            }
            if s.summary.is_none() && *mode != SearchMode::Names {
                out.push_str(" (no summary yet: found by its name and path)");
            }
        }
        result(Ok(out))
    }

    #[tool(
        description = "One symbol of this checkout: its kind, file, lines, signature and summary. `symbol` is a name, `Type::name` or `Type.name`. When a type and its impl blocks have the name, the result is the type. If more than one symbol has the name, the error lists them: give `file`, `line`, or a candidate from the list as `symbol`, to choose one. Give `source: true` to also get the source text of the symbol from the checkout. The text is at most 16384 bytes: the result says when it is cut. If the file changed after the last index update, the result says that the source can be different."
    )]
    async fn symbol(
        &self,
        Parameters(params): Parameters<SymbolParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let (root, index) = self.open_index(params.checkout.as_deref())?;
            let s = find(
                &index,
                &params.symbol,
                params.file.as_deref(),
                params.line,
                SYMBOL,
            )?;
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
                None => out.push_str("\nSummary: none."),
            }
            if s.is_test {
                out.push_str("\nThis is test code.");
            }
            if params.source.unwrap_or(false) {
                out.push_str(&source_text(&s, index.source(&root, &s, SOURCE_LIMIT)?));
            }
            Ok(out)
        })())
    }

    #[tool(
        description = "The functions that call a symbol of this checkout. Give the symbol as for the tool `symbol`, with `file` and `line` when more than one symbol has the name. The result has the certain calls only, unless `possible` is true. A possible call is a call by a name that more than one function has: the result marks it. A call marked \"by name until the next SCIP run\" is in changed code of a language with SCIP (Rust, Go, TypeScript, JavaScript, Python). For each use of the symbol, not only the calls, use `references`. In Terraform code, a reference (`var.x`, `local.x`, `module.z`, `data.t.n`, `t.n`) is a call from the block that holds it to the block that it names, and an argument of a module call is a call to the variable of the module."
    )]
    async fn callers(
        &self,
        Parameters(params): Parameters<EdgeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let (_, index) = self.open_index(params.checkout.as_deref())?;
            let s = find(
                &index,
                &params.symbol,
                params.file.as_deref(),
                params.line,
                SYMBOL,
            )?;
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
                // A call in the body of a macro_rules! definition: each
                // function that the macro makes does the call (KAIROS-T-0353).
                let made = if e.other.kind == "macro" {
                    ": each function that the macro makes does this call"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "\n- {}, call at line {}, {}{made}",
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
        description = "The functions of this checkout that a symbol calls, and the names outside the checkout that it calls. Give the symbol as for the tool `symbol`, with `file` and `line` when more than one symbol has the name. The result has the certain calls only, unless `possible` is true. A possible call has one line for each function with that name, and the result marks it. In Terraform code, a reference (`var.x`, `local.x`, `module.z`, `data.t.n`, `t.n`) is a call from the block that holds it to the block that it names, and an argument of a module call is a call to the variable of the module."
    )]
    async fn callees(
        &self,
        Parameters(params): Parameters<EdgeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let (_, index) = self.open_index(params.checkout.as_deref())?;
            let s = find(
                &index,
                &params.symbol,
                params.file.as_deref(),
                params.line,
                SYMBOL,
            )?;
            let possible = params.possible.unwrap_or(false);
            let kairos_index::Callees {
                edges,
                no_symbol,
                externals,
            } = index.callees(s.id, possible)?;
            let mut out = format!(
                "{} calls {} in the checkout:",
                described(&s),
                plural(edges.len() + no_symbol.len(), "function", "functions")
            );
            // One line for each callee, in the order of the calls. A callee
            // with no symbol (a function that a macro makes) shows the place
            // of its definition (COLLIERY-T-2531).
            let mut lines: Vec<(u32, String)> = edges
                .iter()
                .map(|e| {
                    (
                        e.line,
                        format!(
                            "\n- {}, call at line {}, {}",
                            described(&e.other),
                            e.line,
                            marks(e)
                        ),
                    )
                })
                .collect();
            lines.extend(no_symbol.iter().map(|c| {
                let made = if c.macro_made {
                    "made by a macro there, with no symbol"
                } else {
                    "defined there, with no symbol"
                };
                (
                    c.call_line,
                    format!(
                        "\n- {} ({}:{}, {made}), call at line {}, {}",
                        c.name, c.file, c.line, c.call_line, c.class
                    ),
                )
            }));
            lines.sort_by_key(|(line, _)| *line);
            for (_, line) in lines {
                out.push_str(&line);
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
                let all = index.callees(s.id, true)?.edges.len();
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
        description = "Each use of a symbol of this checkout, not only the calls: in Rust, each use as a type, a field (a field of a type is a use of the type), an import, a call or other (a constant, a module), from the SCIP data of rust-analyzer. Each use has its file, its line, its kind and the symbol that holds it, by file. For Python, TypeScript, JavaScript and Go, the index has the calls only, and the result says so. Give the symbol as for the tool `symbol`, with `file` and `line` when more than one symbol has the name. Possible calls (by a name that more than one function has) are not shown, unless `possible` is true. Give an ADR short code as `symbol` (for example `KAIROS-A-0065`) to get each place that cites the ADR: a comment, a doc string or the name of a test, with its file, its line and its symbol. `limit`: the most uses to give, from 1 to 1000, default 100."
    )]
    async fn references(
        &self,
        Parameters(params): Parameters<ReferencesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let limit = params.limit.unwrap_or(DEFAULT_USES);
            if !(1..=MAX_USES).contains(&limit) {
                return Err(ToolError::Limit(MAX_USES));
            }
            let (_, index) = self.open_index(params.checkout.as_deref())?;
            // KAIROS-T-0703: an ADR short code gives the places that cite it.
            if kairos_index::is_adr_code(&params.symbol) {
                let extra = [
                    ("file", params.file.is_some()),
                    ("line", params.line.is_some()),
                    ("possible", params.possible.is_some()),
                ]
                .into_iter()
                .find(|(_, given)| *given);
                if let Some((field, _)) = extra {
                    return Err(ToolError::AdrCodeArgument {
                        symbol: params.symbol.clone(),
                        field: field.to_string(),
                        others: "`file`, `line` or `possible`".to_string(),
                    });
                }
                return Ok(citations_text(&index, &params.symbol, limit)?);
            }
            let s = find(
                &index,
                &params.symbol,
                params.file.as_deref(),
                params.line,
                SYMBOL,
            )?;
            Ok(references_text(
                &index,
                &s,
                params.possible.unwrap_or(false),
                limit,
            )?)
        })())
    }

    #[tool(
        description = "How one symbol of this checkout reaches another: the 3 shortest chains of certain calls from `from` to `to`, shortest first, each with other steps. Give `from` and `to` as `symbol` of the tool `symbol`, with `from_file`, `from_line`, `to_file` and `to_line` when more than one symbol has a name. Each chain has one call on each line, in order. Possible calls are not followed."
    )]
    async fn path(
        &self,
        Parameters(params): Parameters<PathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let (_, index) = self.open_index(params.checkout.as_deref())?;
            let from = find(
                &index,
                &params.from,
                params.from_file.as_deref(),
                params.from_line,
                FROM,
            )?;
            let to = find(
                &index,
                &params.to,
                params.to_file.as_deref(),
                params.to_line,
                TO,
            )?;
            let chains = index.paths(from.id, to.id, MAX_CHAINS)?;
            if chains.is_empty() {
                return Ok(format!(
                    "No chain of certain calls goes from {} to {}.",
                    described(&from),
                    described(&to)
                ));
            }
            let mut out = format!(
                "{} of certain calls from {} to {}, shortest first:",
                plural(chains.len(), "chain", "chains"),
                described(&from),
                described(&to)
            );
            for (n, steps) in chains.iter().enumerate() {
                out.push_str(&format!(
                    "\n\nChain {}, {}:",
                    n + 1,
                    plural(steps.len(), "call", "calls")
                ));
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
            let (_, index) = self.open_index(params.checkout.as_deref())?;
            Ok(duplicates_text(&index, &options, limit)?)
        })())
    }

    #[tool(
        description = "The map of this checkout: each folder of source and test files, with its summary and its files, each file with its summary. A folder of Terraform code also lists its managed resources by type, its data resources, its module calls with their sources, its variables and its outputs. `[count]` and `[for_each]` mark a block that has them. A folder that a module call of the checkout calls has a `called by:` line. A root folder has a `calls:` line with each chain of module calls. Use it in place of .metis/code-index.md. `under` keeps the folders under one path."
    )]
    async fn module_map(
        &self,
        Parameters(params): Parameters<ModuleMapParams>,
    ) -> Result<CallToolResult, ErrorData> {
        result((|| {
            let (_, index) = self.open_index(params.checkout.as_deref())?;
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
                // The Terraform view of the folder (KAIROS-T-0472).
                for line in m.terraform.iter().flat_map(|t| t.lines()) {
                    out.push('\n');
                    out.push_str(&line);
                }
                for f in &m.files {
                    out.push_str("\n- ");
                    out.push_str(&map_file_line(f));
                }
            }
            Ok(out)
        })())
    }
}

/// The line of one file in the module map, with no `- `: the path and the
/// summary. A file with no summary shows its symbol count, or "declarations
/// only" when it has no symbol to summarize (KAIROS-T-0425).
fn map_file_line(f: &FileInfo) -> String {
    let test = if f.is_test { "test code, " } else { "" };
    match &f.summary {
        Some(summary) => format!(
            "{}{}: {summary}",
            f.path,
            if f.is_test { " (test code)" } else { "" }
        ),
        None if f.declarations_only => format!("{} ({test}declarations only)", f.path),
        None => format!(
            "{} ({test}{})",
            f.path,
            plural(f.symbols, "symbol", "symbols")
        ),
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
                "{} Use these tools before you read files: \
                 `module_map` for the map, `code_search` to find code by what it does, \
                 `symbol` for one symbol, `callers` and `callees` for the call graph, \
                 `references` for each use of a symbol (types, fields, imports and calls) or \
                 each place that cites an ADR, \
                 `path` for how one symbol reaches another, and `duplicates` for repeated \
                 code. `fetch_index` opens a checkout: it takes the nearest index of the \
                 repository from Kairos. More than one checkout can be open: each tool \
                 takes `checkout`, the folder name or the path of an open one, and reads \
                 the last one opened by default. Run `kairos index update` after you change \
                 code.",
                match self.root() {
                    Some(root) => format!("The code index of the checkout {}.", display(&root)),
                    None => no_checkout_instructions(&self.below),
                }
            ))
    }
}

/// The first part of the instructions with no open checkout. The server
/// names the checkouts below its start folder (#272): the agent opens the
/// one it works on, and does not have to find the paths first.
fn no_checkout_instructions(below: &[Found]) -> String {
    let mut out = "The server has no open checkout: call `fetch_index` with the path of the \
                   checkout you work on."
        .to_string();
    if below.is_empty() {
        return out;
    }
    out.push_str(&format!(
        " The start folder is not a checkout. It has {} below it:",
        plural(below.len(), "checkout", "checkouts")
    ));
    for found in below {
        out.push_str(&format!("\n- {}", found.line()));
    }
    out.push_str(
        "\n`fetch_index` finds the repository in Kairos from the origin remote. Call it for \
         each checkout you work on: they stay open, and `code_search` searches each of \
         them.\n",
    );
    out
}

/// The most chains that `path` gives (COLLIERY-T-2531).
const MAX_CHAINS: usize = 3;

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(file: &str, line: u32) -> SymbolInfo {
        SymbolInfo {
            id: i64::from(line),
            file: file.into(),
            name: "ContentHash".into(),
            container: None,
            kind: "implementation".into(),
            language: "rust".into(),
            start_line: line,
            end_line: line + 5,
            signature: None,
            is_test: false,
            summary: None,
        }
    }

    /// #272: a session that starts in a folder above several checkouts.
    /// The server opens none, and names each one, in the instructions and
    /// in the error of a tool that needs an open checkout.
    #[test]
    fn a_start_folder_above_checkouts_names_them() {
        let work = tempfile::tempdir().expect("a temporary folder");
        let work = work.path().canonicalize().expect("the folder");
        for name in ["service", "engine", ".hidden"] {
            std::fs::create_dir_all(work.join(name).join(".git")).expect("a clone");
        }
        std::fs::create_dir_all(work.join("engine-fix")).expect("a worktree");
        std::fs::write(work.join("engine-fix").join(".git"), "gitdir: /nowhere\n")
            .expect("its .git file");
        let engine_db = super::super::db_path(&work.join("engine"));
        std::fs::create_dir_all(engine_db.parent().expect("a folder")).expect("the index folder");
        std::fs::write(&engine_db, b"").expect("a local index");
        std::fs::create_dir_all(work.join("scripts")).expect("a folder of no repository");

        let tools = CodeTools::new(work.clone());
        assert_eq!(tools.root(), None, "a folder above checkouts opens none");
        let names: Vec<&str> = tools.below.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["engine", "engine-fix", "service"]);
        let engine = &tools.below[0];
        assert!(engine.indexed);
        assert_eq!(
            engine.line(),
            format!(
                "engine ({}, no origin remote, a local index)",
                work.join("engine").display()
            )
        );

        let instructions = no_checkout_instructions(&tools.below);
        assert!(
            instructions.contains("It has 3 checkouts below it:\n- engine ("),
            "{instructions}"
        );
        assert!(instructions.contains("\n- service ("), "{instructions}");
        assert!(!instructions.contains("scripts"), "{instructions}");

        let error = tools.no_checkout().text();
        assert!(
            error.starts_with("NO_CHECKOUT: The server has no open checkout."),
            "{error}"
        );
        assert!(
            error.contains(&format!(
                "The checkouts below the start folder of the server: engine ({}, ",
                work.join("engine").display()
            )),
            "{error}"
        );
    }

    /// #272: `checkout` names an open checkout by its folder name or its
    /// path; no `checkout` is the last one opened.
    #[test]
    fn checkout_picks_an_open_checkout_by_name_or_path() {
        let work = tempfile::tempdir().expect("a temporary folder");
        let work = work.path().canonicalize().expect("the folder");
        let tools = CodeTools::new(work.clone());
        assert!(matches!(tools.pick(None), Err(ToolError::NoCheckout(_))));
        let (service, engine, other) = (
            work.join("service"),
            work.join("engine"),
            work.join("other").join("service"),
        );
        for root in [&service, &engine, &other] {
            std::fs::create_dir_all(root).expect("a checkout");
        }
        tools.add_open(service.clone());
        tools.add_open(engine.clone());
        assert_eq!(tools.pick(None).unwrap(), engine, "the last one opened");
        assert_eq!(tools.pick(Some("service")).unwrap(), service);
        assert_eq!(tools.pick(Some(&display(&engine))).unwrap(), engine);
        assert_eq!(
            open_line(&tools.opened()).as_deref(),
            Some(
                "2 checkouts are open: service, engine. Give `checkout` to read another one. \
                 `code_search` with no `checkout` searches each of them."
            )
        );
        // Opened again: it is the default again, and it is open one time.
        tools.add_open(service.clone());
        assert_eq!(tools.opened(), [engine.clone(), service.clone()]);

        let unknown = tools.pick(Some("nowhere")).unwrap_err().text();
        assert!(
            unknown.starts_with("VALIDATION: No open checkout is \"nowhere\"."),
            "{unknown}"
        );
        // 2 open checkouts with one folder name: the path chooses.
        tools.add_open(other.clone());
        let twice = tools.pick(Some("service")).unwrap_err().text();
        assert!(
            twice.starts_with(
                "VALIDATION: More than one open checkout has the folder name \"service\". \
                 Give its path."
            ),
            "{twice}"
        );
        assert_eq!(tools.pick(Some(&display(&other))).unwrap(), other);
    }

    /// A folder with no checkout below it: the text of before #272.
    #[test]
    fn a_start_folder_with_no_checkout_below_names_none() {
        let work = tempfile::tempdir().expect("a temporary folder");
        let tools = CodeTools::new(work.path().to_path_buf());
        assert!(tools.below.is_empty());
        assert_eq!(
            no_checkout_instructions(&tools.below),
            "The server has no open checkout: call `fetch_index` with the path of the \
             checkout you work on."
        );
        assert!(
            tools
                .no_checkout()
                .text()
                .ends_with("is not a repository of Kairos."),
            "{}",
            tools.no_checkout().text()
        );
    }

    /// KAIROS-T-0425: a file with no symbol to summarize and no summary.
    #[test]
    fn a_file_of_declarations_with_no_summary_says_so() {
        let file = |summary: Option<&str>, declarations_only, is_test| FileInfo {
            path: "src/lib.rs".into(),
            summary: summary.map(str::to_string),
            symbols: 3,
            is_test,
            declarations_only,
        };
        assert_eq!(
            map_file_line(&file(None, true, false)),
            "src/lib.rs (declarations only)"
        );
        assert_eq!(
            map_file_line(&file(None, true, true)),
            "src/lib.rs (test code, declarations only)"
        );
        assert_eq!(
            map_file_line(&file(None, false, false)),
            "src/lib.rs (3 symbols)"
        );
        assert_eq!(
            map_file_line(&file(Some("Declares the modules."), true, false)),
            "src/lib.rs: Declares the modules."
        );
    }

    /// KAIROS-T-0420: the source part of `symbol`.
    #[test]
    fn the_source_text_says_when_it_is_cut_or_changed() {
        let s = sym("src/a.rs", 10);
        assert_eq!(
            source_text(&s, None),
            "\nSource: none. The file src/a.rs is not in the checkout now."
        );
        let whole = SymbolSource {
            text: "fn f() {}".into(),
            bytes: 9,
            cut: false,
            changed: false,
        };
        assert_eq!(
            source_text(&s, Some(whole)),
            "\nSource: lines 10-15, 9 bytes.\n```rust\nfn f() {}\n```"
        );
        let cut = SymbolSource {
            text: "/// ```\nfn f() {\n".into(),
            bytes: 20000,
            cut: true,
            changed: true,
        };
        assert_eq!(
            source_text(&s, Some(cut)),
            "\nSource: lines 10-15, 20000 bytes. Warning: the file changed after the last \
             index update. The source can be different from the symbol. Run `kairos index \
             update`. The text is cut at the limit of 16384 bytes: it has 17 of 20000. Read \
             src/a.rs from line 12 for the rest.\n````rust\n/// ```\nfn f() {\n````"
        );
    }

    #[test]
    fn candidates_in_one_file_ask_for_the_line() {
        let all = [sym("src/document.rs", 126), sym("src/document.rs", 168)];
        assert_eq!(
            ambiguous("ContentHash", &all, SYMBOL).text(),
            "AMBIGUOUS: 2 symbols have the name \"ContentHash\". They are in one file. \
             Give the argument `line` to choose one. Or give a candidate from the list as \
             `symbol`.\ndetails: {\"candidates\":[\"ContentHash (src/document.rs:126)\",\
             \"ContentHash (src/document.rs:168)\"]}"
        );
    }

    #[test]
    fn candidates_in_different_files_ask_for_the_file() {
        let all = [sym("src/a.rs", 1), sym("src/b.rs", 1)];
        assert_eq!(
            ambiguous("ContentHash", &all, FROM).to_string(),
            "2 symbols have the name \"ContentHash\". Give the argument `from_file` to choose \
             one. Or give a candidate from the list as `from`."
        );
        let all = [sym("src/a.rs", 1), sym("src/a.rs", 9), sym("src/b.rs", 1)];
        assert_eq!(
            ambiguous("ContentHash", &all, TO).to_string(),
            "3 symbols have the name \"ContentHash\". Give the arguments `to_file` and \
             `to_line` to choose one. Or give a candidate from the list as `to`."
        );
    }

    #[test]
    fn the_not_found_text_quotes_the_name_and_the_file() {
        let text = |file, line| {
            ToolError::NoSymbol {
                name: "ContentHash".into(),
                place: Place::of(file, line),
            }
            .text()
        };
        assert_eq!(
            text(None, None),
            "NOT_FOUND: No symbol has the name \"ContentHash\"."
        );
        assert_eq!(
            text(Some("src/document.rs"), None),
            "NOT_FOUND: No symbol has the name \"ContentHash\" in the file \"src/document.rs\"."
        );
        assert_eq!(
            text(Some("src/document.rs"), Some(12)),
            "NOT_FOUND: No symbol has the name \"ContentHash\" in the file \"src/document.rs\" \
             at line 12."
        );
        assert_eq!(
            text(None, Some(12)),
            "NOT_FOUND: No symbol has the name \"ContentHash\" at line 12."
        );
    }
}
