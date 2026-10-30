//! `KairosClient` — the typed HTTP client for the Kairos API
//! (KAIROS-T-0024, per KAIROS-A-0009/A-0015: one client shared by the CLI,
//! the integration suite, and skills verification).
//!
//! Every endpoint family of KAIROS-S-0005 has a typed method returning the
//! shared DTOs from this crate's `types*` modules; non-2xx responses map to
//! the typed [`Error`] enum. Authentication is a bearer token obtained per
//! request from a [`TokenProvider`] (so the CLI can plug in a refreshing
//! provider); tests use [`StaticToken`]. Tenant resolution defaults to the
//! deployment's Host-based scheme; [`KairosClient::with_tenant`] sets the
//! `X-Tenant` dev/test fallback header on every request.
//!
//! [`KairosClient::raw_request`] is the sanctioned escape hatch for
//! protocol-level probes (malformed bodies, wire-shape assertions) that the
//! typed surface deliberately cannot express.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::Error;
use crate::types::{
    Adr, AppendNoteRequest, AppendNoteResponse, CascadePreviewResponse, CreateAdrRequest,
    CreateDocumentRequest, CreateInitiativeRequest, CreateStrategyRequest, CreateTaskRequest,
    DeleteResponse, Document, ErrorEnvelope, ImpactListQuery, Initiative, ListEnvelope, ListQuery,
    Pagination, PromoteNoteRequest, PromoteNoteResponse, RestoreResponse, SetComplexityRequest,
    SetDocumentBoardRequest, SetLifecycleRequest, SetTaskReviewRequest, SetTaskStateRequest,
    SetWorkClassRequest, Strategy, Task, TransitionRequest, UpdateContentRequest,
};
use crate::types_auth::{DeploymentConfig, LoginRequest, LoginResponse};
use crate::types_meta::{
    ActivityEntry, ActivityQuery, ChildrenProgressResponse, CreateMetadataDefinitionRequest,
    CreateRelationshipRequest, CreateTemplateRequest, DeletedResponse, HistoryQuery,
    HistorySnapshot, HistoryVersion, ItemActivityEntry, ItemActivityQuery, ItemMetadataResponse,
    ItemRelationshipsResponse, MetadataDefinition, ProposedRelationship, Relationship,
    RelationshipOutcome, Template, TemplateDetail, UpdateMetadataDefinitionRequest,
    UpdateMetadataRequest, UpdateTemplateRequest,
};
use crate::types_org::{
    AddBoardMemberRequest, AddOrgMemberRequest, AddStreamTeamRequest, AddTeamMemberRequest,
    BOARD_ITEMS_MAX_LIMIT, Board, BoardColumn, BoardDetail, BoardItemsQuery, BoardItemsResponse,
    BoardMember, BoardTransition, CreateBoardRequest, CreateColumnRequest, CreateStreamRequest,
    CreateStructureGrantRequest, CreateTeamRequest, CreateTenantRequest, CreateTransitionRequest,
    DeliveryStream, OrgDeleteResponse, OrgMember, RemoveBoardMemberResponse,
    RemoveOrgMemberResponse, ReplaceCapabilitiesRequest, StructureGrant, Team, TeamMember,
    TenantCreatedResponse, TenantDeletedResponse, TenantSummary, UpdateBoardRequest,
    UpdateColumnRequest, UpdateOrgMemberRequest, UpdateStreamRequest, UpdateTeamRequest,
    WhoamiResponse,
};
use crate::types_search::{SearchRequest, SearchResponse};
use crate::types_team_pages::{
    CreateTeamAnnouncementRequest, CreateTeamPageRequest, TeamAnnouncement, TeamPage,
    TeamWorkDocument, UpdateTeamPageRequest,
};

/// Supplies the bearer token for each request. The CLI implements this
/// with a refreshing OAuth credential cache (KAIROS-A-0010); tests use
/// [`StaticToken`].
pub trait TokenProvider: Send + Sync {
    /// The bearer token to attach to the next request (WITHOUT the
    /// `Bearer ` prefix).
    fn bearer_token(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>>;
}

/// A fixed, never-refreshed token.
///
/// `Debug` is written by hand (COLLIERY-T-0213): the field is a bearer, and
/// `kairos logout` puts a session bearer in it. A derived `Debug` prints it.
#[derive(Clone)]
pub struct StaticToken(pub String);

impl fmt::Debug for StaticToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("StaticToken")
            .field(&crate::types_auth::REDACTED)
            .finish()
    }
}

impl TokenProvider for StaticToken {
    fn bearer_token(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>> {
        let token = self.0.clone();
        Box::pin(async move { Ok(token) })
    }
}

/// The provider of a client that holds no credential (COLLIERY-T-0213).
///
/// A client that logs in has no bearer yet. Its public calls never ask for
/// one. If a call that needs a bearer reaches this provider, the call fails
/// here and sends nothing, which is better than a request with an empty
/// `Authorization` header.
#[derive(Debug, Clone, Copy)]
struct NoCredential;

impl TokenProvider for NoCredential {
    fn bearer_token(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>> {
        Box::pin(async {
            Err(Error::Token(
                "this client holds no credential; log in first".to_string(),
            ))
        })
    }
}

/// The five S-0005 entity families, for the endpoints that are generic
/// over the family path segment (metadata, relationships, history).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Strategy,
    Initiative,
    Task,
    Document,
    Adr,
}

impl EntityKind {
    /// The URL path segment of the family (`/api/{segment}/...`).
    pub const fn path_segment(self) -> &'static str {
        match self {
            EntityKind::Strategy => "strategies",
            EntityKind::Initiative => "initiatives",
            EntityKind::Task => "tasks",
            EntityKind::Document => "documents",
            EntityKind::Adr => "adrs",
        }
    }
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.path_segment())
    }
}

/// The time the client waits for a connection (KAIROS-T-0607).
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The time the client waits for a whole request, from the start to the
/// end of the body (KAIROS-T-0607). [`KairosClient::with_timeout`] and
/// [`timeout_from_env`] change it.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// The environment variable that holds the request timeout in seconds.
pub const TIMEOUT_VAR: &str = "KAIROS_TIMEOUT";

/// The request timeout that `KAIROS_TIMEOUT` sets: `None` when the
/// variable is not set or is empty. A value that is not a positive integer
/// of seconds is refused with [`Error::Setting`], which names the
/// variable.
pub fn timeout_from_env() -> Result<Option<Duration>, Error> {
    parse_timeout(std::env::var(TIMEOUT_VAR).ok().as_deref())
}

/// [`timeout_from_env`] over a value that is already read.
pub fn parse_timeout(value: Option<&str>) -> Result<Option<Duration>, Error> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    match value.parse::<u64>() {
        Ok(seconds) if seconds > 0 => Ok(Some(Duration::from_secs(seconds))),
        _ => Err(Error::Setting {
            variable: TIMEOUT_VAR,
            message: format!(
                "The value {value:?} of {TIMEOUT_VAR} is not a positive integer. Give a number of \
                 seconds, for example {TIMEOUT_VAR}=120."
            ),
        }),
    }
}

/// The HTTP client with the two timeouts. Compression is on: reqwest sends
/// `Accept-Encoding: gzip, br` and decodes the answer (KAIROS-T-0607).
fn build_http(connect_timeout: Duration, timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .build()
        .expect("the HTTP client builds: the TLS backend is in the binary")
}

/// The typed Kairos API client. Cheap to clone.
#[derive(Clone)]
pub struct KairosClient {
    pub(crate) http: reqwest::Client,
    /// Base URL without a trailing slash (`http://host:port`).
    pub(crate) base_url: String,
    pub(crate) token: Arc<dyn TokenProvider>,
    /// `X-Tenant` header value (dev/test tenant resolution fallback).
    pub(crate) tenant: Option<String>,
    connect_timeout: Duration,
    timeout: Duration,
}

impl fmt::Debug for KairosClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KairosClient")
            .field("base_url", &self.base_url)
            .field("tenant", &self.tenant)
            .field("connect_timeout", &self.connect_timeout)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl KairosClient {
    /// A client against `base_url` drawing tokens from `token`, with the
    /// default timeouts ([`DEFAULT_CONNECT_TIMEOUT`], [`DEFAULT_TIMEOUT`]).
    pub fn new(base_url: impl Into<String>, token: Arc<dyn TokenProvider>) -> Self {
        let mut base_url = base_url.into();
        while base_url.ends_with('/') {
            base_url.pop();
        }
        Self {
            http: build_http(DEFAULT_CONNECT_TIMEOUT, DEFAULT_TIMEOUT),
            base_url,
            token,
            tenant: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Wait `timeout` for a whole request (KAIROS-T-0607).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self.http = build_http(self.connect_timeout, self.timeout);
        self
    }

    /// Wait `connect_timeout` for a connection (KAIROS-T-0607).
    #[must_use]
    pub fn with_connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.connect_timeout = connect_timeout;
        self.http = build_http(self.connect_timeout, self.timeout);
        self
    }

    /// The request timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// The connection timeout.
    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    /// A client with a fixed bearer token (integration tests).
    pub fn with_static_token(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self::new(base_url, Arc::new(StaticToken(token.into())))
    }

    /// A client that holds no credential, for the calls that come before a
    /// login: [`KairosClient::deployment_config`] and [`KairosClient::login`]
    /// (COLLIERY-T-0213).
    pub fn anonymous(base_url: impl Into<String>) -> Self {
        Self::new(base_url, Arc::new(NoCredential))
    }

    /// Send `X-Tenant: {slug}` on every request (dev/test tenant
    /// resolution, KAIROS-A-0005 §2).
    #[must_use]
    pub fn with_tenant(mut self, slug: impl Into<String>) -> Self {
        self.tenant = Some(slug.into());
        self
    }

    /// The configured base URL (no trailing slash).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The configured `X-Tenant` value, if any.
    pub fn tenant(&self) -> Option<&str> {
        self.tenant.as_deref()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// Attach the bearer token and the optional X-Tenant header.
    async fn authorize(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, Error> {
        let token = self.token.bearer_token().await?;
        let mut builder = builder.bearer_auth(token);
        if let Some(tenant) = &self.tenant {
            builder = builder.header("x-tenant", tenant);
        }
        Ok(builder)
    }

    /// Send an authorized request; decode the EXACT expected success
    /// status into `T` (the client doubles as the S-0005 contract harness,
    /// so 200-where-201-was-promised is an error, not a pass), map
    /// enveloped rejections through [`Error::from_envelope`].
    async fn execute<T: DeserializeOwned>(
        &self,
        context: String,
        expect: u16,
        builder: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        let builder = self.authorize(builder).await?;
        Self::send_and_decode(context, expect, builder).await
    }

    /// Attach the optional X-Tenant header and NO bearer token: the public
    /// routes that a client calls before it holds a credential
    /// (COLLIERY-T-0213).
    fn without_credential(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.tenant {
            Some(tenant) => builder.header("x-tenant", tenant),
            None => builder,
        }
    }

    /// Send a prepared request and decode the response. Shared by the
    /// authorized path and the public path, so that the two map a rejection
    /// in the same way.
    async fn send_and_decode<T: DeserializeOwned>(
        context: String,
        expect: u16,
        builder: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        let response = builder.send().await?;
        let status = response.status().as_u16();
        let body = response.text().await?;
        if status == expect {
            serde_json::from_str(&body).map_err(|source| Error::Decode { context, source })
        } else {
            match serde_json::from_str::<ErrorEnvelope>(&body) {
                Ok(envelope) if !(200..300).contains(&status) => {
                    Err(Error::from_envelope(status, envelope))
                }
                _ => Err(Error::UnexpectedResponse { status, body }),
            }
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        self.execute(format!("GET {path}"), 200, self.http.get(self.url(path)))
            .await
    }

    async fn get_query<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &(impl Serialize + ?Sized),
    ) -> Result<T, Error> {
        self.execute(
            format!("GET {path}"),
            200,
            self.http.get(self.url(path)).query(query),
        )
        .await
    }

    /// POST expecting 201 Created (the S-0005 create family).
    async fn post_created<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &(impl Serialize + ?Sized),
    ) -> Result<T, Error> {
        self.execute(
            format!("POST {path}"),
            201,
            self.http.post(self.url(path)).json(body),
        )
        .await
    }

    /// POST expecting 200 OK (actions on existing resources, e.g.
    /// transitions and search).
    async fn post_ok<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &(impl Serialize + ?Sized),
    ) -> Result<T, Error> {
        self.execute(
            format!("POST {path}"),
            200,
            self.http.post(self.url(path)).json(body),
        )
        .await
    }

    /// POST with no body, expecting 200 OK (restore, rotate). The server
    /// refuses a body on a route that accepts none (COLLIERY-T-0256), and
    /// `{}` and `null` are bodies.
    async fn post_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        self.execute(format!("POST {path}"), 200, self.http.post(self.url(path)))
            .await
    }

    /// A `POST` with no body that answers 201.
    async fn post_empty_created<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        self.execute(format!("POST {path}"), 201, self.http.post(self.url(path)))
            .await
    }

    async fn patch<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &(impl Serialize + ?Sized),
    ) -> Result<T, Error> {
        self.execute(
            format!("PATCH {path}"),
            200,
            self.http.patch(self.url(path)).json(body),
        )
        .await
    }

    /// PUT expecting 200 OK (whole-field replacement, e.g. a task's
    /// repository binding).
    async fn put_ok<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &(impl Serialize + ?Sized),
    ) -> Result<T, Error> {
        self.execute(
            format!("PUT {path}"),
            200,
            self.http.put(self.url(path)).json(body),
        )
        .await
    }

    async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        self.execute(
            format!("DELETE {path}"),
            200,
            self.http.delete(self.url(path)),
        )
        .await
    }

    /// Protocol-level escape hatch: send an authorized request with an
    /// arbitrary JSON body and return `(status, raw JSON body)` WITHOUT
    /// typed decoding or error mapping. For probes the typed surface
    /// cannot express (malformed bodies, unknown path families,
    /// wire-shape assertions such as "empty groups are omitted").
    pub async fn raw_request(
        &self,
        method: Method,
        path_and_query: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), Error> {
        let mut builder = self.http.request(method, self.url(path_and_query));
        if let Some(body) = body {
            builder = builder.json(body);
        }
        let response = self.authorize(builder).await?.send().await?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await?;
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|source| Error::Decode {
                context: format!("raw {path_and_query}"),
                source,
            })?
        };
        Ok((status, value))
    }
}

/// The five entity CRUD families (S-0005; KAIROS-T-0018 contracts).
macro_rules! entity_family {
    ($family:literal, $dto:ty, $create_req:ty,
     $list:ident, $get:ident, $create:ident, $update:ident, $delete:ident,
     $restore:ident) => {
        entity_family!(
            $family, $dto, $create_req, ListQuery, $list, $get, $create, $update, $delete,
            $restore
        );
    };
    // COLLIERY-T-0269: the list of documents and the list of ADRs take
    // `ImpactListQuery`, which has the filter `repository`. Each caller
    // that gives a `Pagination` or a `ListQuery` gets what it got before.
    ($family:literal, $dto:ty, $create_req:ty, $query:ty,
     $list:ident, $get:ident, $create:ident, $update:ident, $delete:ident,
     $restore:ident) => {
        #[doc = concat!("`GET /api/", $family, "` — list (tenant-open read).\n\n",
                                "Takes a [`Pagination`] for the live-only listing or a ",
                                "[`ListQuery`] to widen it to archived work (KAIROS-T-0159); ",
                                "`total` always counts whatever the page shows.")]
        pub async fn $list(
            &self,
            query: impl Into<$query>,
        ) -> Result<ListEnvelope<$dto>, Error> {
            self.get_query(concat!("/api/", $family), &query.into())
                .await
        }

        #[doc = concat!("`GET /api/", $family, "/{short_code}`.")]
        pub async fn $get(&self, short_code: &str) -> Result<$dto, Error> {
            self.get(&format!(concat!("/api/", $family, "/{}"), short_code))
                .await
        }

        #[doc = concat!("`POST /api/", $family, "`.")]
        pub async fn $create(&self, request: &$create_req) -> Result<$dto, Error> {
            self.post_created(concat!("/api/", $family), request).await
        }

        #[doc = concat!("`PATCH /api/", $family, "/{short_code}` — the KAIROS-A-0004 ",
                                "optimistic-concurrency content edit (stale version = 409).")]
        pub async fn $update(
            &self,
            short_code: &str,
            request: &UpdateContentRequest,
        ) -> Result<$dto, Error> {
            self.patch(
                &format!(concat!("/api/", $family, "/{}"), short_code),
                request,
            )
            .await
        }

        #[doc = concat!("`DELETE /api/", $family, "/{short_code}` — soft delete with ",
                                "the KAIROS-A-0001 cascade report.")]
        pub async fn $delete(&self, short_code: &str) -> Result<DeleteResponse, Error> {
            self.delete(&format!(concat!("/api/", $family, "/{}"), short_code))
                .await
        }

        #[doc = concat!("`POST /api/", $family, "/{short_code}/restore` — put archived ",
                                "work back (KAIROS-A-0020). 422 `RESTORE_BLOCKED` when its ",
                                "board, column, team or repository is gone.")]
        pub async fn $restore(&self, short_code: &str) -> Result<RestoreResponse, Error> {
            self.post_empty(&format!(
                concat!("/api/", $family, "/{}/restore"),
                short_code
            ))
            .await
        }
    };
}

/// `POST /api/{family}/{short_code}/transition` for the on-board families.
macro_rules! entity_transition {
    ($family:literal, $dto:ty, $transition:ident) => {
        #[doc = concat!("`POST /api/", $family, "/{short_code}/transition` — move to a ",
                                        "reachable column (422 `INVALID_TRANSITION` otherwise).")]
        pub async fn $transition(
            &self,
            short_code: &str,
            to_column_id: &str,
        ) -> Result<$dto, Error> {
            self.post_ok(
                &format!(concat!("/api/", $family, "/{}/transition"), short_code),
                &TransitionRequest {
                    to_column_id: to_column_id.to_string(),
                },
            )
            .await
        }
    };
}

impl KairosClient {
    // -- entities (KAIROS-T-0018) ------------------------------------------
    entity_family!(
        "strategies",
        Strategy,
        CreateStrategyRequest,
        list_strategies,
        get_strategy,
        create_strategy,
        update_strategy,
        delete_strategy,
        restore_strategy
    );
    entity_transition!("strategies", Strategy, transition_strategy);

    entity_family!(
        "initiatives",
        Initiative,
        CreateInitiativeRequest,
        list_initiatives,
        get_initiative,
        create_initiative,
        update_initiative,
        delete_initiative,
        restore_initiative
    );
    entity_transition!("initiatives", Initiative, transition_initiative);

    /// `PUT /api/initiatives/{short_code}/complexity` — set the complexity
    /// of an initiative; `none` clears it. The edit rule applies.
    pub async fn set_initiative_complexity(
        &self,
        short_code: &str,
        complexity: &str,
    ) -> Result<Initiative, Error> {
        self.put_ok(
            &format!("/api/initiatives/{short_code}/complexity"),
            &SetComplexityRequest {
                complexity: complexity.to_string(),
            },
        )
        .await
    }

    entity_family!(
        "tasks",
        Task,
        CreateTaskRequest,
        list_tasks,
        get_task,
        create_task,
        update_task,
        delete_task,
        restore_task
    );
    entity_transition!("tasks", Task, transition_task);

    /// `POST /api/tasks/{short_code}/work-class` — move a task between
    /// the Planned/Support lanes (KAIROS-T-0077). Orthogonal to column
    /// transitions; requires `transition_items` on the task's board.
    pub async fn set_task_work_class(
        &self,
        short_code: &str,
        work_class: &str,
    ) -> Result<Task, Error> {
        self.post_ok(
            &format!("/api/tasks/{short_code}/work-class"),
            &SetWorkClassRequest {
                work_class: work_class.to_string(),
            },
        )
        .await
    }

    /// `POST /api/tasks/{short_code}/move` — move a task to another delivery
    /// board (slug or UUID); it lands in the entry column (KAIROS-I-0012).
    /// The task keeps its repository (COLLIERY-T-0217).
    pub async fn move_task(&self, short_code: &str, board: &str) -> Result<Task, Error> {
        self.move_task_with(short_code, board, false).await
    }

    /// [`Self::move_task`], and with `rename` the task also gets the next
    /// code of the target board (COLLIERY-T-3101). The response has the new
    /// code.
    pub async fn move_task_with(
        &self,
        short_code: &str,
        board: &str,
        rename: bool,
    ) -> Result<Task, Error> {
        self.post_ok(
            &format!("/api/tasks/{short_code}/move"),
            &crate::types::MoveTaskRequest {
                board: board.to_string(),
                rename,
            },
        )
        .await
    }

    /// `POST /api/{family}/{short_code}/notes/{note_id}/promote` — turn a
    /// note into a draft ADR or document (KAIROS-T-0505).
    pub async fn promote_note(
        &self,
        kind: EntityKind,
        short_code: &str,
        note_id: &str,
        request: &PromoteNoteRequest,
    ) -> Result<PromoteNoteResponse, Error> {
        self.post_created(
            &format!("/api/{kind}/{short_code}/notes/{note_id}/promote"),
            request,
        )
        .await
    }

    /// `PUT /api/tasks/{short_code}/review` — set the review state of a
    /// task (KAIROS-T-0718): `not_needed`, `needed` or `done`.
    pub async fn set_task_review(
        &self,
        short_code: &str,
        review: &SetTaskReviewRequest,
    ) -> Result<Task, Error> {
        self.put_ok(&format!("/api/tasks/{short_code}/review"), review)
            .await
    }

    /// `PUT /api/tasks/{short_code}/state` — write the live state of a task
    /// (KAIROS-T-0506). The write is a patch (KAIROS-T-0533): a field that
    /// is not sent keeps its value, `null` clears it.
    pub async fn set_task_state(
        &self,
        short_code: &str,
        state: &SetTaskStateRequest,
    ) -> Result<Task, Error> {
        self.put_ok(&format!("/api/tasks/{short_code}/state"), state)
            .await
    }

    /// `POST /api/tasks/{short_code}/claim` — take the claim of a task in
    /// Active that has no claim (KAIROS-T-0528). The caller holds
    /// `transition_items` on the board of the task.
    /// A task that a different person holds is refused with `CLAIMED`.
    pub async fn claim_task(&self, short_code: &str) -> Result<Task, Error> {
        self.post_empty(&format!("/api/tasks/{short_code}/claim"))
            .await
    }

    /// `POST /api/tasks/{short_code}/hand-off` — give the claim of a task
    /// in Active to a person (KAIROS-T-0359). `to` is a user id, an email
    /// or a user name of a person of the organization, or `me`. The caller has the
    /// claim, or holds `transition_items` on the board of the task. The
    /// person must hold `transition_items` on that board too (KAIROS-T-0530).
    pub async fn hand_off_task(&self, short_code: &str, to: &str) -> Result<Task, Error> {
        self.post_ok(
            &format!("/api/tasks/{short_code}/hand-off"),
            &crate::types::HandOffRequest { to: to.to_string() },
        )
        .await
    }

    /// `GET /api/tasks/{short_code}/hand-off-targets` — the persons who
    /// can get the claim of the task: each person who holds
    /// `transition_items` on its board (KAIROS-T-0530).
    pub async fn hand_off_targets(
        &self,
        short_code: &str,
    ) -> Result<Vec<crate::types::HandOffTarget>, Error> {
        self.get(&format!("/api/tasks/{short_code}/hand-off-targets"))
            .await
    }

    /// `POST /api/tasks/{short_code}/release` — end the claim of a task in
    /// Active (KAIROS-T-0359). The task stays in Active, free for anyone.
    /// The caller has the claim, or holds `transition_items` on the board
    /// of the task.
    pub async fn release_task(&self, short_code: &str) -> Result<Task, Error> {
        self.post_empty(&format!("/api/tasks/{short_code}/release"))
            .await
    }

    /// `POST /api/tasks/{short_code}/cancel` — cancel a task with a reason
    /// (KAIROS-T-0362). The task moves to the done column of its board and
    /// gets the cancel mark. The caller holds `transition_items` on the
    /// board of the task.
    pub async fn cancel_task(&self, short_code: &str, reason: &str) -> Result<Task, Error> {
        self.post_ok(
            &format!("/api/tasks/{short_code}/cancel"),
            &crate::types::CancelTaskRequest {
                reason: reason.to_string(),
            },
        )
        .await
    }

    /// `POST /api/tasks/{short_code}/reopen` — reopen a done task
    /// (KAIROS-T-0564). The task moves to the first column of its board
    /// that is not done, and its cancel mark ends. The caller holds
    /// `transition_items` on the board of the task.
    pub async fn reopen_task(&self, short_code: &str) -> Result<Task, Error> {
        self.post_empty(&format!("/api/tasks/{short_code}/reopen"))
            .await
    }

    /// `POST /api/tasks/{short_code}/purge` — delete a task for good
    /// (KAIROS-T-0362). Nothing can bring it back. The caller holds
    /// `manage_tasks` on the board of the task.
    pub async fn purge_task(
        &self,
        short_code: &str,
    ) -> Result<crate::types::PurgeTaskResponse, Error> {
        self.post_empty(&format!("/api/tasks/{short_code}/purge"))
            .await
    }

    /// `POST /api/boards/{id}/archive-completed` — archive each task in the
    /// done columns of a board (KAIROS-T-0363). `board` is a slug or a
    /// UUID. The caller holds `manage_tasks` on the board.
    pub async fn archive_completed(
        &self,
        board: &str,
    ) -> Result<crate::types::ArchiveCompletedResponse, Error> {
        self.post_empty(&format!("/api/boards/{board}/archive-completed"))
            .await
    }

    /// `POST /api/boards/{id}/archive-completed?older_than_days=N`: archive
    /// each task of the done columns that entered its column at least N
    /// days ago (KAIROS-T-0649). `None` archives each done task, as
    /// [`Client::archive_completed`].
    pub async fn archive_completed_older_than(
        &self,
        board: &str,
        older_than_days: Option<i64>,
    ) -> Result<crate::types::ArchiveCompletedResponse, Error> {
        match older_than_days {
            Some(days) => {
                self.post_empty(&format!(
                    "/api/boards/{board}/archive-completed?older_than_days={days}"
                ))
                .await
            }
            None => self.archive_completed(board).await,
        }
    }

    /// `PUT /api/tasks/{short_code}/repository` — set the repository the
    /// task links to (slug or UUID) or clear it with `None` (KAIROS-T-0104).
    /// It can be any live repository, of any team, and the board and the
    /// team of the task do not change (COLLIERY-T-0217, COLLIERY-A-0023).
    /// The edit rule applies (COLLIERY-T-0228): the caller created the
    /// task, or holds `manage_tasks` on its board, or is an org admin.
    pub async fn set_task_repository(
        &self,
        short_code: &str,
        repository: Option<&str>,
    ) -> Result<Task, Error> {
        self.put_ok(
            &format!("/api/tasks/{short_code}/repository"),
            &crate::types_repositories::SetTaskRepositoryRequest {
                repository: repository.map(str::to_string),
            },
        )
        .await
    }

    /// `GET /api/boards/{id}/items?repository=` — the board narrowed to
    /// tasks bound to one repository (slug or UUID, KAIROS-T-0104); other
    /// entity types are unaffected. Each item, page after page
    /// (COLLIERY-T-0261).
    pub async fn board_items_for_repository(
        &self,
        board_id: &str,
        repository: &str,
    ) -> Result<BoardItemsResponse, Error> {
        self.board_items_all(
            board_id,
            &BoardItemsQuery {
                repository: Some(repository.to_string()),
                ..BoardItemsQuery::default()
            },
        )
        .await
    }

    entity_family!(
        "documents",
        Document,
        CreateDocumentRequest,
        ImpactListQuery,
        list_documents,
        get_document,
        create_document,
        update_document,
        delete_document,
        restore_document
    );

    entity_family!(
        "adrs",
        Adr,
        CreateAdrRequest,
        ImpactListQuery,
        list_adrs,
        get_adr,
        create_adr,
        update_adr,
        delete_adr,
        restore_adr
    );
    entity_transition!("adrs", Adr, transition_adr);

    // -- boards + configuration + members (KAIROS-T-0019) -------------------

    /// `GET /api/boards`.
    pub async fn list_boards(&self, page: Pagination) -> Result<ListEnvelope<Board>, Error> {
        self.get_query("/api/boards", &page).await
    }

    /// `POST /api/boards` — create a board seeded from the system defaults
    /// for its level (KAIROS-A-0002).
    pub async fn create_board(&self, request: &CreateBoardRequest) -> Result<BoardDetail, Error> {
        self.post_created("/api/boards", request).await
    }

    /// `GET /api/boards/{id}` — board + columns + transition graph.
    /// `board_id` is the slug or the id of the board (COLLIERY-T-0265), as
    /// for each function that reads a board. A function that changes a
    /// board takes the id only.
    pub async fn get_board(&self, board_id: &str) -> Result<BoardDetail, Error> {
        self.get(&format!("/api/boards/{board_id}")).await
    }

    /// `PATCH /api/boards/{id}` — board settings.
    pub async fn update_board(
        &self,
        board_id: &str,
        request: &UpdateBoardRequest,
    ) -> Result<Board, Error> {
        self.patch(&format!("/api/boards/{board_id}"), request)
            .await
    }

    /// `DELETE /api/boards/{id}` (only when empty).
    pub async fn delete_board(&self, board_id: &str) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!("/api/boards/{board_id}")).await
    }

    /// `GET /api/boards/{id}/items` — ONE page of the items, grouped by
    /// column (COLLIERY-T-0261). The default of `limit` is 200 and the
    /// maximum is 1000. `total` of the response is the number of items on
    /// all pages. For each item of the board, use
    /// [`Client::board_items_all`]. `board_id` is the slug or the id of
    /// the board (COLLIERY-T-0265).
    pub async fn board_items(
        &self,
        board_id: &str,
        query: &BoardItemsQuery,
    ) -> Result<BoardItemsResponse, Error> {
        self.get_query(&format!("/api/boards/{board_id}/items"), query)
            .await
    }

    /// `GET /api/boards/{id}/flow` — the flow metrics of a board for a
    /// range of days (KAIROS-T-0372). `board` is a slug or a UUID.
    pub async fn board_flow(
        &self,
        board: &str,
        query: &crate::types_org::BoardFlowQuery,
    ) -> Result<crate::types_org::BoardFlow, Error> {
        self.get_query(&format!("/api/boards/{board}/flow"), query)
            .await
    }

    /// `GET /api/boards/{id}/webhook-targets` (KAIROS-T-0384).
    pub async fn list_webhook_targets(
        &self,
        board: &str,
    ) -> Result<Vec<crate::types_dispatch::WebhookTarget>, Error> {
        self.get(&format!("/api/boards/{board}/webhook-targets"))
            .await
    }

    /// `POST /api/boards/{id}/webhook-targets`: the target and its secret,
    /// shown this one time.
    pub async fn create_webhook_target(
        &self,
        board: &str,
        request: &crate::types_dispatch::CreateWebhookTargetRequest,
    ) -> Result<crate::types_dispatch::CreatedWebhookTarget, Error> {
        self.post_created(&format!("/api/boards/{board}/webhook-targets"), request)
            .await
    }

    /// `DELETE /api/boards/{id}/webhook-targets/{target_id}`.
    pub async fn delete_webhook_target(
        &self,
        board: &str,
        target_id: &str,
    ) -> Result<crate::types_dispatch::WebhookTarget, Error> {
        self.delete(&format!("/api/boards/{board}/webhook-targets/{target_id}"))
            .await
    }

    /// `POST /api/boards/{id}/webhook-targets/{target_id}/redeliver`
    /// (KAIROS-T-0577): queue the `skipped` deliveries of a target that is
    /// on again. `redelivered` of the answer counts them.
    pub async fn redeliver_webhook_target(
        &self,
        board: &str,
        target_id: &str,
        query: &crate::types_dispatch::RedeliverQuery,
    ) -> Result<crate::types_dispatch::WebhookTarget, Error> {
        let path = format!("/api/boards/{board}/webhook-targets/{target_id}/redeliver");
        self.execute(
            format!("POST {path}"),
            200,
            self.http.post(self.url(&path)).query(query),
        )
        .await
    }

    /// `POST /api/boards/{id}/webhook-targets/{target_id}/enable`. The
    /// answer counts the `skipped` deliveries that Kairos queued again
    /// (`redelivered`, KAIROS-T-0577).
    pub async fn enable_webhook_target(
        &self,
        board: &str,
        target_id: &str,
    ) -> Result<crate::types_dispatch::WebhookTarget, Error> {
        self.post_empty(&format!(
            "/api/boards/{board}/webhook-targets/{target_id}/enable"
        ))
        .await
    }

    /// `POST /api/boards/{id}/webhook-targets/{target_id}/test`: a `ping`
    /// delivery.
    pub async fn test_webhook_target(
        &self,
        board: &str,
        target_id: &str,
    ) -> Result<crate::types_dispatch::WebhookDelivery, Error> {
        self.post_empty_created(&format!(
            "/api/boards/{board}/webhook-targets/{target_id}/test"
        ))
        .await
    }

    /// `GET /api/boards/{id}/dispatch-rules` (KAIROS-T-0385).
    pub async fn list_dispatch_rules(
        &self,
        board: &str,
    ) -> Result<Vec<crate::types_dispatch::DispatchRule>, Error> {
        self.get(&format!("/api/boards/{board}/dispatch-rules"))
            .await
    }

    /// `GET /api/boards/{id}/agent-triage` (KAIROS-T-0428).
    pub async fn get_agent_triage(
        &self,
        board: &str,
    ) -> Result<crate::types_dispatch::AgentTriage, Error> {
        self.get(&format!("/api/boards/{board}/agent-triage")).await
    }

    /// `PUT /api/boards/{id}/agent-triage` (KAIROS-T-0428).
    pub async fn set_agent_triage(
        &self,
        board: &str,
        agents_decide: bool,
    ) -> Result<crate::types_dispatch::AgentTriage, Error> {
        self.put_ok(
            &format!("/api/boards/{board}/agent-triage"),
            &crate::types_dispatch::SetAgentTriageRequest { agents_decide },
        )
        .await
    }

    /// `POST /api/boards/{id}/dispatch-rules`.
    pub async fn create_dispatch_rule(
        &self,
        board: &str,
        request: &crate::types_dispatch::CreateDispatchRuleRequest,
    ) -> Result<crate::types_dispatch::DispatchRule, Error> {
        self.post_created(&format!("/api/boards/{board}/dispatch-rules"), request)
            .await
    }

    /// `DELETE /api/boards/{id}/dispatch-rules/{rule_id}`.
    pub async fn delete_dispatch_rule(
        &self,
        board: &str,
        rule_id: &str,
    ) -> Result<crate::types_dispatch::DispatchRule, Error> {
        self.delete(&format!("/api/boards/{board}/dispatch-rules/{rule_id}"))
            .await
    }

    /// `GET /api/boards/{id}/webhook-deliveries`: the delivery log.
    pub async fn list_webhook_deliveries(
        &self,
        board: &str,
        query: &crate::types_dispatch::WebhookDeliveriesQuery,
    ) -> Result<Vec<crate::types_dispatch::WebhookDelivery>, Error> {
        self.get_query(&format!("/api/boards/{board}/webhook-deliveries"), query)
            .await
    }

    /// Each item of a board: `GET /api/boards/{id}/items`, page after
    /// page, as one response (COLLIERY-T-0261). `limit` and `offset` of
    /// `query` have no effect: the pages have the maximum size, and the
    /// first page is at offset 0.
    pub async fn board_items_all(
        &self,
        board_id: &str,
        query: &BoardItemsQuery,
    ) -> Result<BoardItemsResponse, Error> {
        let page_query = |offset: i64| BoardItemsQuery {
            limit: Some(BOARD_ITEMS_MAX_LIMIT),
            offset: Some(offset),
            ..query.clone()
        };
        let mut all = self.board_items(board_id, &page_query(0)).await?;
        loop {
            let read = i64::try_from(all.item_count()).unwrap_or(i64::MAX);
            if read >= all.total {
                break;
            }
            let page = self.board_items(board_id, &page_query(read)).await?;
            // A page with no item: the board got smaller between two
            // pages. Stop, so that the loop has an end.
            if page.item_count() == 0 {
                all.total = page.total;
                break;
            }
            all.add_page(page);
        }
        all.limit = i64::try_from(all.item_count()).unwrap_or(i64::MAX);
        all.offset = 0;
        Ok(all)
    }

    /// `GET /api/boards/{id}/items?include_deleted=true` — the board as it
    /// was, archived cards included and each marked with `archived_at`
    /// (KAIROS-A-0020 rule 2). Not a board view: the put-away cards still
    /// carry the column they were put away in, so this answers "what was
    /// in Done last quarter?", not "what is on the board now?". Each
    /// item, page after page (COLLIERY-T-0261).
    pub async fn board_items_including_archived(
        &self,
        board_id: &str,
    ) -> Result<BoardItemsResponse, Error> {
        self.board_items_all(
            board_id,
            &BoardItemsQuery {
                include_deleted: true,
                ..BoardItemsQuery::default()
            },
        )
        .await
    }

    /// `GET /api/boards/{id}/columns`. `board_id` is the slug or the id.
    pub async fn list_columns(&self, board_id: &str) -> Result<Vec<BoardColumn>, Error> {
        self.get(&format!("/api/boards/{board_id}/columns")).await
    }

    /// `POST /api/boards/{id}/columns`.
    pub async fn add_column(
        &self,
        board_id: &str,
        request: &CreateColumnRequest,
    ) -> Result<BoardColumn, Error> {
        self.post_created(&format!("/api/boards/{board_id}/columns"), request)
            .await
    }

    /// `PATCH /api/boards/{id}/columns/{col_id}` — rename and/or move.
    pub async fn update_column(
        &self,
        board_id: &str,
        column_id: &str,
        request: &UpdateColumnRequest,
    ) -> Result<BoardColumn, Error> {
        self.patch(
            &format!("/api/boards/{board_id}/columns/{column_id}"),
            request,
        )
        .await
    }

    /// `DELETE /api/boards/{id}/columns/{col_id}` (only when empty).
    pub async fn remove_column(
        &self,
        board_id: &str,
        column_id: &str,
    ) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!("/api/boards/{board_id}/columns/{column_id}"))
            .await
    }

    /// `GET /api/boards/{id}/transitions`. `board_id` is the slug or the
    /// id.
    pub async fn list_transitions(&self, board_id: &str) -> Result<Vec<BoardTransition>, Error> {
        self.get(&format!("/api/boards/{board_id}/transitions"))
            .await
    }

    /// `POST /api/boards/{id}/transitions`.
    pub async fn add_transition(
        &self,
        board_id: &str,
        request: &CreateTransitionRequest,
    ) -> Result<BoardTransition, Error> {
        self.post_created(&format!("/api/boards/{board_id}/transitions"), request)
            .await
    }

    /// `DELETE /api/boards/{id}/transitions/{transition_id}`.
    pub async fn remove_transition(
        &self,
        board_id: &str,
        transition_id: &str,
    ) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!(
            "/api/boards/{board_id}/transitions/{transition_id}"
        ))
        .await
    }

    /// `GET /api/boards/{id}/members` — members + capability grants.
    /// `board_id` is the slug or the id.
    pub async fn list_board_members(&self, board_id: &str) -> Result<Vec<BoardMember>, Error> {
        self.get(&format!("/api/boards/{board_id}/members")).await
    }

    /// `POST /api/boards/{id}/members` — grant capabilities
    /// (KAIROS-A-0006).
    pub async fn add_board_member(
        &self,
        board_id: &str,
        request: &AddBoardMemberRequest,
    ) -> Result<BoardMember, Error> {
        self.post_created(&format!("/api/boards/{board_id}/members"), request)
            .await
    }

    /// `PATCH /api/boards/{id}/members/{user_id}` — replace the full
    /// capability set.
    pub async fn replace_capabilities(
        &self,
        board_id: &str,
        user_id: &str,
        request: &ReplaceCapabilitiesRequest,
    ) -> Result<BoardMember, Error> {
        self.patch(
            &format!("/api/boards/{board_id}/members/{user_id}"),
            request,
        )
        .await
    }

    /// `DELETE /api/boards/{id}/members/{user_id}` — revoke everything.
    pub async fn remove_board_member(
        &self,
        board_id: &str,
        user_id: &str,
    ) -> Result<RemoveBoardMemberResponse, Error> {
        self.delete(&format!("/api/boards/{board_id}/members/{user_id}"))
            .await
    }

    // -- the structure capability (KAIROS-T-0337) -----------------------------

    /// `GET /api/structure-grants` — each grant of `manage_structure`.
    pub async fn list_structure_grants(&self) -> Result<Vec<StructureGrant>, Error> {
        self.get("/api/structure-grants").await
    }

    /// `POST /api/structure-grants` — grant `manage_structure` at the
    /// organization (no `board`) or on one board. Org admin only.
    pub async fn create_structure_grant(
        &self,
        request: &CreateStructureGrantRequest,
    ) -> Result<StructureGrant, Error> {
        self.post_created("/api/structure-grants", request).await
    }

    /// `DELETE /api/structure-grants/{id}` — revoke one grant. Org admin
    /// only.
    pub async fn delete_structure_grant(&self, grant_id: &str) -> Result<StructureGrant, Error> {
        self.delete(&format!("/api/structure-grants/{grant_id}"))
            .await
    }

    // -- teams ---------------------------------------------------------------

    /// `GET /api/teams`.
    pub async fn list_teams(&self, page: Pagination) -> Result<ListEnvelope<Team>, Error> {
        self.get_query("/api/teams", &page).await
    }

    /// `POST /api/teams` — creates the team AND its delivery board
    /// (KAIROS-A-0002).
    pub async fn create_team(&self, request: &CreateTeamRequest) -> Result<Team, Error> {
        self.post_created("/api/teams", request).await
    }

    /// `GET /api/teams/{id}`.
    pub async fn get_team(&self, team_id: &str) -> Result<Team, Error> {
        self.get(&format!("/api/teams/{team_id}")).await
    }

    /// `PATCH /api/teams/{id}`.
    pub async fn update_team(
        &self,
        team_id: &str,
        request: &UpdateTeamRequest,
    ) -> Result<Team, Error> {
        self.patch(&format!("/api/teams/{team_id}"), request).await
    }

    /// `DELETE /api/teams/{id}`.
    pub async fn delete_team(&self, team_id: &str) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!("/api/teams/{team_id}")).await
    }

    /// `GET /api/teams/{id}/members`.
    pub async fn list_team_members(&self, team_id: &str) -> Result<Vec<TeamMember>, Error> {
        self.get(&format!("/api/teams/{team_id}/members")).await
    }

    /// `POST /api/teams/{id}/members`.
    pub async fn add_team_member(
        &self,
        team_id: &str,
        request: &AddTeamMemberRequest,
    ) -> Result<TeamMember, Error> {
        self.post_created(&format!("/api/teams/{team_id}/members"), request)
            .await
    }

    /// `DELETE /api/teams/{id}/members/{user_id}`.
    pub async fn remove_team_member(
        &self,
        team_id: &str,
        user_id: &str,
    ) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!("/api/teams/{team_id}/members/{user_id}"))
            .await
    }

    // -- delivery streams ------------------------------------------------------

    /// `GET /api/delivery-streams`.
    pub async fn list_streams(
        &self,
        page: Pagination,
    ) -> Result<ListEnvelope<DeliveryStream>, Error> {
        self.get_query("/api/delivery-streams", &page).await
    }

    /// `POST /api/delivery-streams`.
    pub async fn create_stream(
        &self,
        request: &CreateStreamRequest,
    ) -> Result<DeliveryStream, Error> {
        self.post_created("/api/delivery-streams", request).await
    }

    /// `GET /api/delivery-streams/{id}`.
    pub async fn get_stream(&self, stream_id: &str) -> Result<DeliveryStream, Error> {
        self.get(&format!("/api/delivery-streams/{stream_id}"))
            .await
    }

    /// `PATCH /api/delivery-streams/{id}`.
    pub async fn update_stream(
        &self,
        stream_id: &str,
        request: &UpdateStreamRequest,
    ) -> Result<DeliveryStream, Error> {
        self.patch(&format!("/api/delivery-streams/{stream_id}"), request)
            .await
    }

    /// `DELETE /api/delivery-streams/{id}`.
    pub async fn delete_stream(&self, stream_id: &str) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!("/api/delivery-streams/{stream_id}"))
            .await
    }

    /// `GET /api/delivery-streams/{id}/teams`.
    pub async fn list_stream_teams(&self, stream_id: &str) -> Result<Vec<Team>, Error> {
        self.get(&format!("/api/delivery-streams/{stream_id}/teams"))
            .await
    }

    /// `POST /api/delivery-streams/{id}/teams`.
    pub async fn add_stream_team(
        &self,
        stream_id: &str,
        request: &AddStreamTeamRequest,
    ) -> Result<OrgDeleteResponse, Error> {
        self.post_created(&format!("/api/delivery-streams/{stream_id}/teams"), request)
            .await
    }

    /// `DELETE /api/delivery-streams/{id}/teams/{team_id}`.
    pub async fn remove_stream_team(
        &self,
        stream_id: &str,
        team_id: &str,
    ) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!(
            "/api/delivery-streams/{stream_id}/teams/{team_id}"
        ))
        .await
    }

    // -- service accounts + API keys (/api/service-accounts, A-0017) -----------

    /// `POST /api/service-accounts` — create a machine principal (org-admin).
    pub async fn create_service_account(
        &self,
        request: &crate::types_service_accounts::CreateServiceAccountRequest,
    ) -> Result<crate::types_service_accounts::ServiceAccount, Error> {
        self.post_created("/api/service-accounts", request).await
    }

    /// `GET /api/service-accounts` — list the org's service accounts.
    pub async fn list_service_accounts(
        &self,
    ) -> Result<crate::types_service_accounts::ServiceAccountList, Error> {
        self.get("/api/service-accounts").await
    }

    /// `DELETE /api/service-accounts/{id}` — delete a service account + its keys.
    pub async fn delete_service_account(
        &self,
        id: &str,
    ) -> Result<crate::types_service_accounts::Deleted, Error> {
        self.delete(&format!("/api/service-accounts/{id}")).await
    }

    /// `POST /api/service-accounts/{id}/keys` — mint a key (raw returned once).
    pub async fn create_api_key(
        &self,
        service_account_id: &str,
        request: &crate::types_service_accounts::CreateApiKeyRequest,
    ) -> Result<crate::types_service_accounts::ApiKeyCreated, Error> {
        self.post_created(
            &format!("/api/service-accounts/{service_account_id}/keys"),
            request,
        )
        .await
    }

    /// `GET /api/service-accounts/{id}/keys` — list a service account's keys.
    pub async fn list_api_keys(
        &self,
        service_account_id: &str,
    ) -> Result<crate::types_service_accounts::ApiKeyList, Error> {
        self.get(&format!("/api/service-accounts/{service_account_id}/keys"))
            .await
    }

    /// `DELETE /api/service-accounts/{id}/keys/{key_id}` — revoke a key.
    pub async fn revoke_api_key(
        &self,
        service_account_id: &str,
        key_id: &str,
    ) -> Result<crate::types_service_accounts::Deleted, Error> {
        self.delete(&format!(
            "/api/service-accounts/{service_account_id}/keys/{key_id}"
        ))
        .await
    }

    // -- agent keys of each person (/api/org/agent-keys, KAIROS-T-0388) -------

    /// `GET /api/org/agent-keys` — a page of the agent keys of the persons
    /// of the organization (never a secret). Org-admin.
    pub async fn list_org_agent_keys(
        &self,
        query: &crate::types_service_accounts::OrgAgentKeysQuery,
    ) -> Result<crate::types::ListEnvelope<crate::types_service_accounts::OrgAgentKey>, Error> {
        self.get_query("/api/org/agent-keys", query).await
    }

    /// `DELETE /api/org/agent-keys/{key_id}` — revoke the agent key of any
    /// person. Org-admin.
    pub async fn revoke_org_agent_key(
        &self,
        key_id: &str,
    ) -> Result<crate::types_service_accounts::Deleted, Error> {
        self.delete(&format!("/api/org/agent-keys/{key_id}")).await
    }

    // -- agent keys of the caller (/api/me/agent-keys, KAIROS-T-0359) ----------

    /// `POST /api/me/agent-keys` — make an agent key that acts as the caller.
    /// The response is the only place the raw key appears.
    pub async fn create_agent_key(
        &self,
        request: &crate::types_service_accounts::CreateApiKeyRequest,
    ) -> Result<crate::types_service_accounts::ApiKeyCreated, Error> {
        self.post_created("/api/me/agent-keys", request).await
    }

    /// `GET /api/me/agent-keys` — the caller's agent keys (never a secret).
    pub async fn list_agent_keys(
        &self,
    ) -> Result<crate::types_service_accounts::ApiKeyList, Error> {
        self.get("/api/me/agent-keys").await
    }

    /// `GET /api/me/notification-settings` (KAIROS-T-0381): each kind of
    /// notification, and whether it is on for the caller.
    pub async fn notification_settings(
        &self,
    ) -> Result<crate::types_notifications::NotificationSettings, Error> {
        self.get("/api/me/notification-settings").await
    }

    /// `POST /api/notices` (KAIROS-T-0737): send a notice to a person or
    /// to each member of a team.
    pub async fn send_notice(
        &self,
        request: &crate::types_notifications::SendNoticeRequest,
    ) -> Result<crate::types_notifications::SendNoticeResponse, Error> {
        self.post_created("/api/notices", request).await
    }

    /// `PUT /api/me/notification-settings` (KAIROS-T-0381): turn kinds on
    /// or off for the caller.
    pub async fn update_notification_settings(
        &self,
        request: &crate::types_notifications::UpdateNotificationSettingsRequest,
    ) -> Result<crate::types_notifications::NotificationSettings, Error> {
        self.put_ok("/api/me/notification-settings", request).await
    }

    /// `GET /api/me/notifications` (KAIROS-T-0382): the unread
    /// notifications of the caller, then the newest read ones.
    pub async fn notifications(
        &self,
    ) -> Result<crate::types_notifications::NotificationList, Error> {
        self.get("/api/me/notifications").await
    }

    /// `GET /api/me/notifications?before=` (KAIROS-T-0550): the next page
    /// of the unread notifications of the caller, older than `before`.
    pub async fn notifications_before(
        &self,
        before: &str,
    ) -> Result<crate::types_notifications::NotificationList, Error> {
        self.get(&format!("/api/me/notifications?before={before}"))
            .await
    }

    /// `GET /api/me/notifications?read_before=` (KAIROS-T-0584): the next
    /// page of the read notifications of the caller, read before
    /// `read_before`.
    pub async fn notifications_read_before(
        &self,
        read_before: &str,
    ) -> Result<crate::types_notifications::NotificationList, Error> {
        self.get(&format!("/api/me/notifications?read_before={read_before}"))
            .await
    }

    /// `GET /api/me/work` (KAIROS-T-0489): the work of the caller, in two
    /// lanes (initiatives and tasks).
    pub async fn my_work(&self) -> Result<crate::types_work::MyWork, Error> {
        self.get("/api/me/work").await
    }

    /// `GET /api/teams/{id}/standup` (KAIROS-T-0722): the team and its
    /// people, for a stand-up.
    pub async fn standup(&self, team: &str) -> Result<crate::types_work::Standup, Error> {
        self.get(&format!("/api/teams/{team}/standup")).await
    }

    /// `GET /api/teams/{id}/standup/people/{user_id}` (KAIROS-T-0722): the
    /// work of one person in the team.
    pub async fn standup_person(
        &self,
        team: &str,
        user_id: &str,
    ) -> Result<crate::types_work::MyWork, Error> {
        self.get(&format!("/api/teams/{team}/standup/people/{user_id}"))
            .await
    }

    /// `GET /api/teams/{id}/standup/unclaimed` (KAIROS-T-0722): the tasks of
    /// the team that no one claims.
    pub async fn standup_unclaimed(
        &self,
        team: &str,
    ) -> Result<crate::types_work::UnclaimedWork, Error> {
        self.get(&format!("/api/teams/{team}/standup/unclaimed"))
            .await
    }

    /// `POST /api/me/notifications/read` (KAIROS-T-0382): mark notifications
    /// of the caller read.
    pub async fn mark_notifications_read(
        &self,
        request: &crate::types_notifications::MarkNotificationsReadRequest,
    ) -> Result<crate::types_notifications::MarkNotificationsReadResponse, Error> {
        self.post_ok("/api/me/notifications/read", request).await
    }

    /// `DELETE /api/me/agent-keys/{key_id}` — revoke one of the caller's keys.
    pub async fn revoke_agent_key(
        &self,
        key_id: &str,
    ) -> Result<crate::types_service_accounts::Deleted, Error> {
        self.delete(&format!("/api/me/agent-keys/{key_id}")).await
    }

    /// `POST /api/me/agent-keys/{key_id}/rotate` (KAIROS-T-0370): a new
    /// secret for the key; the old one ends after the grace time.
    pub async fn rotate_agent_key(
        &self,
        key_id: &str,
    ) -> Result<crate::types_service_accounts::ApiKeyRotated, Error> {
        self.post_empty_created(&format!("/api/me/agent-keys/{key_id}/rotate"))
            .await
    }

    /// `POST /api/service-accounts/{id}/keys/{key_id}/rotate`
    /// (KAIROS-T-0370), org-admin.
    pub async fn rotate_api_key(
        &self,
        service_account: &str,
        key_id: &str,
    ) -> Result<crate::types_service_accounts::ApiKeyRotated, Error> {
        self.post_empty_created(&format!(
            "/api/service-accounts/{service_account}/keys/{key_id}/rotate"
        ))
        .await
    }

    // -- organization membership (/api/members) --------------------------------

    /// `GET /api/members`.
    pub async fn list_org_members(
        &self,
        page: Pagination,
    ) -> Result<ListEnvelope<OrgMember>, Error> {
        self.get_query("/api/members", &page).await
    }

    /// `POST /api/members` — add by email (the user must have logged in
    /// once; 404 otherwise).
    pub async fn add_org_member(&self, request: &AddOrgMemberRequest) -> Result<OrgMember, Error> {
        self.post_created("/api/members", request).await
    }

    /// `PATCH /api/members/{user_id}` — role change (422 `LAST_ADMIN`
    /// guard).
    pub async fn update_org_member(
        &self,
        user_id: &str,
        request: &UpdateOrgMemberRequest,
    ) -> Result<OrgMember, Error> {
        self.patch(&format!("/api/members/{user_id}"), request)
            .await
    }

    /// `DELETE /api/members/{user_id}`.
    pub async fn remove_org_member(&self, user_id: &str) -> Result<RemoveOrgMemberResponse, Error> {
        self.delete(&format!("/api/members/{user_id}")).await
    }

    // -- deployment-admin tenant provisioning (/api/admin/tenants) --------------

    /// `GET /api/admin/tenants` (deployment-admin only; cross-tenant, no
    /// X-Tenant needed).
    pub async fn list_tenants(
        &self,
        page: Pagination,
    ) -> Result<ListEnvelope<TenantSummary>, Error> {
        self.get_query("/api/admin/tenants", &page).await
    }

    /// `POST /api/admin/tenants` — provision a tenant (KAIROS-T-0008).
    pub async fn create_tenant(
        &self,
        request: &CreateTenantRequest,
    ) -> Result<TenantCreatedResponse, Error> {
        self.post_created("/api/admin/tenants", request).await
    }

    /// `DELETE /api/admin/tenants/{slug}` — destructive; requires
    /// `confirm = Some(true)` (422 `CONFIRMATION_REQUIRED` otherwise;
    /// `None` sends no `confirm` parameter at all).
    pub async fn delete_tenant(
        &self,
        slug: &str,
        confirm: Option<bool>,
    ) -> Result<TenantDeletedResponse, Error> {
        let path = format!("/api/admin/tenants/{slug}");
        let mut builder = self.http.delete(self.url(&path));
        if let Some(confirm) = confirm {
            builder = builder.query(&[("confirm", confirm)]);
        }
        self.execute(format!("DELETE {path}"), 200, builder).await
    }

    // -- relationships (KAIROS-T-0020) ------------------------------------------

    /// `GET /api/{family}/{short_code}/relationships` — both directions,
    /// grouped by type.
    pub async fn relationships(
        &self,
        kind: EntityKind,
        short_code: &str,
    ) -> Result<ItemRelationshipsResponse, Error> {
        self.get(&format!("/api/{kind}/{short_code}/relationships"))
            .await
    }

    /// `PATCH /api/documents/{short_code}/lifecycle` — set a document's
    /// editorial lifecycle (KAIROS-T-0078): draft | review | published |
    /// archived, free transitions, no version bump.
    pub async fn set_document_lifecycle(
        &self,
        short_code: &str,
        lifecycle: &str,
    ) -> Result<Document, Error> {
        self.patch(
            &format!("/api/documents/{short_code}/lifecycle"),
            &SetLifecycleRequest {
                lifecycle: lifecycle.to_string(),
            },
        )
        .await
    }

    /// `PATCH /api/documents/{short_code}/board` — change the owner board
    /// of a document (COLLIERY-T-0269). `board` is a slug or a UUID. The
    /// board cannot be removed (COLLIERY-T-3109): an empty `board` is
    /// refused with 422 `VALIDATION`. The caller needs `manage_documents`
    /// on the board that owns the document now and on the new board.
    pub async fn set_document_board(
        &self,
        short_code: &str,
        board: &str,
    ) -> Result<Document, Error> {
        self.set_document_board_with(short_code, board, false).await
    }

    /// [`Self::set_document_board`], and with `rename` the document also
    /// gets the next code of its new owner board (COLLIERY-T-3101). A
    /// rename needs a new board.
    pub async fn set_document_board_with(
        &self,
        short_code: &str,
        board: &str,
        rename: bool,
    ) -> Result<Document, Error> {
        self.patch(
            &format!("/api/documents/{short_code}/board"),
            &SetDocumentBoardRequest {
                board: board.to_string(),
                rename,
            },
        )
        .await
    }

    /// `GET /api/{family}/{short_code}/teams` — the teams of an initiative
    /// or a strategy: from its tasks and set by hand (KAIROS-T-0321).
    pub async fn item_teams(
        &self,
        kind: EntityKind,
        short_code: &str,
    ) -> Result<crate::types_org::ItemTeamsResponse, Error> {
        self.get(&format!("/api/{kind}/{short_code}/teams")).await
    }

    /// `POST /api/{family}/{short_code}/teams` — set a team (slug or UUID)
    /// on an initiative or a strategy by hand (KAIROS-T-0321). The edit
    /// rule of the item applies.
    pub async fn set_item_team(
        &self,
        kind: EntityKind,
        short_code: &str,
        team: &str,
    ) -> Result<crate::types_org::ItemTeam, Error> {
        self.post_created(
            &format!("/api/{kind}/{short_code}/teams"),
            &crate::types_org::SetItemTeamRequest {
                team: team.to_string(),
            },
        )
        .await
    }

    /// `DELETE /api/{family}/{short_code}/teams/{team}` — clear a team that
    /// is set on an initiative or a strategy by hand (KAIROS-T-0321).
    pub async fn clear_item_team(
        &self,
        kind: EntityKind,
        short_code: &str,
        team: &str,
    ) -> Result<crate::types_org::ClearedItemTeamResponse, Error> {
        self.delete(&format!("/api/{kind}/{short_code}/teams/{team}"))
            .await
    }

    /// `GET /api/{family}/{short_code}/impacts` — the repositories that a
    /// document or an ADR impacts (COLLIERY-T-0269).
    pub async fn item_impacts(
        &self,
        kind: EntityKind,
        short_code: &str,
    ) -> Result<crate::types_repositories::ItemImpactsResponse, Error> {
        self.get(&format!("/api/{kind}/{short_code}/impacts")).await
    }

    /// `POST /api/{family}/{short_code}/impacts` — say that a document or
    /// an ADR impacts a repository (slug or UUID, COLLIERY-T-0269). The
    /// edit rule of the item applies, and no right on the repository is
    /// needed. The link gives no right.
    pub async fn add_impact(
        &self,
        kind: EntityKind,
        short_code: &str,
        repository: &str,
    ) -> Result<crate::types_repositories::Impact, Error> {
        self.post_created(
            &format!("/api/{kind}/{short_code}/impacts"),
            &crate::types_repositories::CreateImpactRequest {
                repository: repository.to_string(),
            },
        )
        .await
    }

    /// `DELETE /api/{family}/{short_code}/impacts/{repository}` — remove
    /// an `impacts` link (COLLIERY-T-0269).
    pub async fn remove_impact(
        &self,
        kind: EntityKind,
        short_code: &str,
        repository: &str,
    ) -> Result<crate::types_repositories::DeletedImpactResponse, Error> {
        self.delete(&format!("/api/{kind}/{short_code}/impacts/{repository}"))
            .await
    }

    /// `GET /api/{family}/{short_code}/external` — the external references
    /// of an item (KAIROS-T-0508).
    pub async fn item_external_refs(
        &self,
        kind: EntityKind,
        short_code: &str,
    ) -> Result<crate::types_meta::ItemExternalRefsResponse, Error> {
        self.get(&format!("/api/{kind}/{short_code}/external"))
            .await
    }

    /// `POST /api/{family}/{short_code}/external` — link an item to a URI
    /// with a label (KAIROS-T-0508). The edit rule of the item applies.
    pub async fn add_external_ref(
        &self,
        kind: EntityKind,
        short_code: &str,
        uri: &str,
        label: Option<&str>,
    ) -> Result<crate::types_meta::ExternalRef, Error> {
        self.post_created(
            &format!("/api/{kind}/{short_code}/external"),
            &crate::types_meta::CreateExternalRefRequest {
                uri: uri.to_string(),
                label: label.map(str::to_string),
            },
        )
        .await
    }

    /// `DELETE /api/{family}/{short_code}/external/{id}` — remove an
    /// external reference by its id (KAIROS-T-0508).
    pub async fn remove_external_ref(
        &self,
        kind: EntityKind,
        short_code: &str,
        id: &str,
    ) -> Result<crate::types_meta::DeletedExternalRefResponse, Error> {
        self.delete(&format!("/api/{kind}/{short_code}/external/{id}"))
            .await
    }

    /// `GET /api/{family}/{short_code}/children-progress` — the direct
    /// `parent`-edge children grouped by board column, with the
    /// `(done, total)` summary (KAIROS-T-0080).
    pub async fn children_progress(
        &self,
        kind: EntityKind,
        short_code: &str,
    ) -> Result<ChildrenProgressResponse, Error> {
        self.get(&format!("/api/{kind}/{short_code}/children-progress"))
            .await
    }

    /// `GET /api/{family}/{short_code}/links` — the branches and
    /// pull/merge requests linked to one item (KAIROS-T-0100).
    pub async fn item_links(
        &self,
        kind: EntityKind,
        short_code: &str,
    ) -> Result<Vec<crate::types_forge::ItemLink>, Error> {
        self.get(&format!("/api/{kind}/{short_code}/links")).await
    }

    /// `GET /api/teams/{id}/links` — the team's in-flight forge links
    /// (KAIROS-T-0101). `states` defaults to open+draft server-side.
    pub async fn team_links(
        &self,
        team_id: &str,
        states: Option<&str>,
    ) -> Result<Vec<crate::types_forge::TeamLink>, Error> {
        let path = match states {
            Some(states) => format!("/api/teams/{team_id}/links?state={states}"),
            None => format!("/api/teams/{team_id}/links"),
        };
        self.get(&path).await
    }

    /// `POST /api/repositories/{slug}/import-issues` — start the import of
    /// the open issues of a repository (KAIROS-T-0483, org admin). The
    /// answer is 202 with the job (KAIROS-T-0487): a new one, or the one
    /// that waits or runs.
    pub async fn import_issues(
        &self,
        reference: &str,
    ) -> Result<crate::types_repositories::IssueImport, Error> {
        let path = format!("/api/repositories/{reference}/import-issues");
        self.execute(format!("POST {path}"), 202, self.http.post(self.url(&path)))
            .await
    }

    /// `GET /api/repositories/{slug}/import-issues` — the last import of
    /// the open issues of a repository (KAIROS-T-0487, org admin).
    pub async fn issue_import(
        &self,
        reference: &str,
    ) -> Result<crate::types_repositories::IssueImport, Error> {
        self.get(&format!("/api/repositories/{reference}/import-issues"))
            .await
    }

    /// `GET /api/teams/{id}/unlinked-refs` — the in-flight branches and
    /// pull requests of the repositories of the team that no item is linked
    /// to (KAIROS-T-0480). `states` defaults to open+draft server-side.
    pub async fn team_unlinked_refs(
        &self,
        team_id: &str,
        states: Option<&str>,
    ) -> Result<Vec<crate::types_forge::ForgeRef>, Error> {
        let path = match states {
            Some(states) => format!("/api/teams/{team_id}/unlinked-refs?state={states}"),
            None => format!("/api/teams/{team_id}/unlinked-refs"),
        };
        self.get(&path).await
    }

    /// `GET /api/forge-connections` (KAIROS-T-0097).
    pub async fn list_forge_connections(
        &self,
    ) -> Result<Vec<crate::types_forge::ForgeConnection>, Error> {
        self.get("/api/forge-connections").await
    }

    /// `GET /api/forge-connections/{id}`.
    pub async fn get_forge_connection(
        &self,
        id: &str,
    ) -> Result<crate::types_forge::ForgeConnection, Error> {
        self.get(&format!("/api/forge-connections/{id}")).await
    }

    /// `POST /api/forge-connections` (org admin) — the response carries
    /// `webhook_secret`, which is shown exactly once.
    pub async fn create_forge_connection(
        &self,
        request: &crate::types_forge::CreateForgeConnectionRequest,
    ) -> Result<crate::types_forge::CreatedForgeConnection, Error> {
        self.post_created("/api/forge-connections", request).await
    }

    // -- repositories (KAIROS-T-0106, A-0019) ----------------------------

    /// `GET /api/repositories[?team=&limit=&offset=]` — one page of the
    /// repository directory (open tenant-wide), optionally one team's
    /// (UUID or slug). The page has the shape of each other list
    /// (KAIROS-T-0268).
    pub async fn list_repositories_page(
        &self,
        team: Option<&str>,
        page: Pagination,
    ) -> Result<ListEnvelope<crate::types_repositories::Repository>, Error> {
        #[derive(Serialize)]
        struct Query<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            team: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            offset: Option<i64>,
        }
        self.get_query(
            "/api/repositories",
            &Query {
                team,
                limit: page.limit,
                offset: page.offset,
            },
        )
        .await
    }

    /// The full repository directory, optionally one team's: each page of
    /// [`Self::list_repositories_page`].
    pub async fn list_repositories(
        &self,
        team: Option<&str>,
    ) -> Result<Vec<crate::types_repositories::Repository>, Error> {
        let mut all = Vec::new();
        loop {
            let page = self
                .list_repositories_page(
                    team,
                    Pagination {
                        limit: Some(200),
                        offset: Some(all.len() as i64),
                    },
                )
                .await?;
            let got = page.items.len();
            all.extend(page.items);
            if got == 0 || all.len() as i64 >= page.total {
                return Ok(all);
            }
        }
    }

    /// `GET /api/repositories/{slug}` — the repository, its webhook
    /// connection id, its in-flight links, and the live documents and
    /// ADRs that impact it (COLLIERY-T-0269).
    pub async fn get_repository(
        &self,
        reference: &str,
    ) -> Result<crate::types_repositories::RepositoryDetail, Error> {
        self.get(&format!("/api/repositories/{reference}")).await
    }

    /// `GET /api/repositories/{slug}?include_deleted=true` —
    /// [`Self::get_repository`], with the archived documents and ADRs that
    /// impact the repository too, each marked with `archived_at`
    /// (COLLIERY-T-0269).
    pub async fn get_repository_with_archived(
        &self,
        reference: &str,
    ) -> Result<crate::types_repositories::RepositoryDetail, Error> {
        self.get_query(
            &format!("/api/repositories/{reference}"),
            &crate::types_repositories::RepositoryDetailQuery {
                include_deleted: true,
            },
        )
        .await
    }

    /// `POST /api/repositories` — register a repository under its owning
    /// team (org admin, or a member of that team).
    pub async fn create_repository(
        &self,
        request: &crate::types_repositories::CreateRepositoryRequest,
    ) -> Result<crate::types_repositories::Repository, Error> {
        self.post_created("/api/repositories", request).await
    }

    /// `PATCH /api/repositories/{slug}`.
    pub async fn update_repository(
        &self,
        reference: &str,
        request: &crate::types_repositories::UpdateRepositoryRequest,
    ) -> Result<crate::types_repositories::Repository, Error> {
        self.patch(&format!("/api/repositories/{reference}"), request)
            .await
    }

    /// `DELETE /api/repositories/{slug}` (org admin; 409 while referenced).
    pub async fn delete_repository(&self, reference: &str) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!("/api/repositories/{reference}")).await
    }

    /// `PUT /api/repositories/{slug}/site` — publish the built site of a
    /// repository: one `.tar.gz`, with its MD5 in base64 as `Content-MD5`
    /// (KAIROS-T-0406). The answer is the new live publish.
    pub async fn publish_site(
        &self,
        reference: &str,
        archive: Vec<u8>,
        content_md5: &str,
    ) -> Result<crate::types_sites::SitePublish, Error> {
        let path = format!("/api/repositories/{reference}/site");
        let builder = self
            .http
            .put(self.url(&path))
            .header(reqwest::header::CONTENT_TYPE, "application/gzip")
            .header("content-md5", content_md5)
            .body(archive);
        let response = self.authorize(builder).await?.send().await?;
        let status = response.status().as_u16();
        let body = response.text().await?;
        if status == 201 {
            return serde_json::from_str(&body).map_err(|source| Error::Decode {
                context: format!("PUT {path}"),
                source,
            });
        }
        match serde_json::from_str::<ErrorEnvelope>(&body) {
            Ok(envelope) => Err(Error::from_envelope(status, envelope)),
            Err(_) => Err(Error::UnexpectedResponse { status, body }),
        }
    }

    /// `DELETE /api/repositories/{slug}/site` — unpublish the site of a
    /// repository (KAIROS-T-0406).
    pub async fn unpublish_site(&self, reference: &str) -> Result<(), Error> {
        let path = format!("/api/repositories/{reference}/site");
        let builder = self.http.delete(self.url(&path));
        let response = self.authorize(builder).await?.send().await?;
        let status = response.status().as_u16();
        if status == 204 {
            return Ok(());
        }
        let body = response.text().await?;
        match serde_json::from_str::<ErrorEnvelope>(&body) {
            Ok(envelope) => Err(Error::from_envelope(status, envelope)),
            Err(_) => Err(Error::UnexpectedResponse { status, body }),
        }
    }

    /// `GET /api/repositories/{slug}/write-token` — the status of the
    /// GitHub write-back (KAIROS-T-0386). It never has the token.
    pub async fn get_repository_write_back(
        &self,
        reference: &str,
    ) -> Result<crate::types_repositories::RepositoryWriteBack, Error> {
        self.get(&format!("/api/repositories/{reference}/write-token"))
            .await
    }

    /// `PUT /api/repositories/{slug}/write-token` — set or replace the
    /// write token of the GitHub write-back.
    pub async fn set_repository_write_token(
        &self,
        reference: &str,
        token: &crate::types_auth::Secret,
    ) -> Result<crate::types_repositories::RepositoryWriteBack, Error> {
        self.put_ok(
            &format!("/api/repositories/{reference}/write-token"),
            &crate::types_repositories::SetRepositoryCredentialRequest {
                token: token.clone(),
            },
        )
        .await
    }

    /// `DELETE /api/repositories/{slug}/write-token` — remove the write
    /// token: Kairos writes nothing more to GitHub.
    pub async fn remove_repository_write_token(
        &self,
        reference: &str,
    ) -> Result<crate::types_repositories::RepositoryWriteBack, Error> {
        self.delete(&format!("/api/repositories/{reference}/write-token"))
            .await
    }

    /// `GET /api/repositories/{slug}/credential` — the status of the read
    /// token of a repository (COLLIERY-T-3105). It never has the token.
    pub async fn get_repository_credential(
        &self,
        reference: &str,
    ) -> Result<crate::types_repositories::RepositoryCredential, Error> {
        self.get(&format!("/api/repositories/{reference}/credential"))
            .await
    }

    /// `PUT /api/repositories/{slug}/credential` — set or replace the read
    /// token of a repository (org admin, or a member of the owner team).
    pub async fn set_repository_credential(
        &self,
        reference: &str,
        token: &crate::types_auth::Secret,
    ) -> Result<crate::types_repositories::RepositoryCredential, Error> {
        self.put_ok(
            &format!("/api/repositories/{reference}/credential"),
            &crate::types_repositories::SetRepositoryCredentialRequest {
                token: token.clone(),
            },
        )
        .await
    }

    /// `DELETE /api/repositories/{slug}/credential` — remove the read
    /// token of a repository.
    pub async fn remove_repository_credential(
        &self,
        reference: &str,
    ) -> Result<crate::types_repositories::RepositoryCredential, Error> {
        self.delete(&format!("/api/repositories/{reference}/credential"))
            .await
    }

    /// `POST /api/repositories/{slug}/credential/check` — test the read
    /// token with `git ls-remote`, and keep the result.
    pub async fn check_repository_credential(
        &self,
        reference: &str,
    ) -> Result<crate::types_repositories::RepositoryCredential, Error> {
        self.post_empty(&format!("/api/repositories/{reference}/credential/check"))
            .await
    }

    /// `GET /api/repositories/{slug}/code-indexes` — each indexed commit
    /// of a repository (COLLIERY-T-1853).
    pub async fn list_code_indexes(
        &self,
        reference: &str,
    ) -> Result<Vec<crate::types_code_index::CodeIndex>, Error> {
        self.get(&format!("/api/repositories/{reference}/code-indexes"))
            .await
    }

    /// `GET /api/repositories/{slug}/code-indexes/commits` — one page of
    /// the indexed commits of a repository, newest first (KAIROS-T-0518).
    pub async fn code_indexes_page(
        &self,
        reference: &str,
        page: Pagination,
    ) -> Result<ListEnvelope<crate::types_code_index::CodeIndex>, Error> {
        self.get_query(
            &format!("/api/repositories/{reference}/code-indexes/commits"),
            &page,
        )
        .await
    }

    /// `GET /api/repositories/{slug}/code-indexes/builds` — the runs of
    /// the code index builder for a repository, newest first
    /// (KAIROS-T-0331). `limit`: 20 when None, 100 at most.
    pub async fn list_code_index_builds(
        &self,
        reference: &str,
        limit: Option<i64>,
    ) -> Result<crate::types_code_index::CodeIndexBuildList, Error> {
        let query = match limit {
            Some(limit) => format!("?limit={limit}"),
            None => String::new(),
        };
        self.get(&format!(
            "/api/repositories/{reference}/code-indexes/builds{query}"
        ))
        .await
    }

    /// `GET /api/repositories/{slug}/code-indexes/builds` with `limit` and
    /// `offset` (KAIROS-T-0584): one page of the runs, newest first.
    pub async fn code_index_builds_page(
        &self,
        reference: &str,
        query: &crate::types_code_index::CodeIndexBuildListQuery,
    ) -> Result<crate::types_code_index::CodeIndexBuildList, Error> {
        self.get_query(
            &format!("/api/repositories/{reference}/code-indexes/builds"),
            query,
        )
        .await
    }

    /// `GET /api/admin/jobs` — the job queue of the organization and of
    /// the deployment: the jobs by kind and state, and the last failures
    /// (KAIROS-T-0576, an organization admin).
    pub async fn admin_jobs(&self) -> Result<crate::types_org::JobQueueStatus, Error> {
        self.get("/api/admin/jobs").await
    }

    /// `GET /api/org/code-index-settings` — the provider settings of the
    /// code index of the tenant (KAIROS-T-0339); the secrets as a status.
    pub async fn code_index_settings(
        &self,
    ) -> Result<crate::types_code_index::CodeIndexSettings, Error> {
        self.get("/api/org/code-index-settings").await
    }

    /// `PUT /api/org/code-index-settings` — set the provider settings of
    /// the code index (an organization admin, KAIROS-T-0339).
    pub async fn put_code_index_settings(
        &self,
        body: &crate::types_code_index::PutCodeIndexSettings,
    ) -> Result<crate::types_code_index::CodeIndexSettings, Error> {
        self.put_ok("/api/org/code-index-settings", body).await
    }

    /// `GET /api/org/read-tokens` — the read tokens of the organization,
    /// one for each forge host (KAIROS-T-0431, an organization admin). The
    /// host, who set it and when; never the token.
    pub async fn org_read_tokens(
        &self,
    ) -> Result<Vec<crate::types_code_index::OrgReadToken>, Error> {
        self.get("/api/org/read-tokens").await
    }

    /// `PUT /api/org/read-tokens/{host}` — set or replace the read token of
    /// the organization for `host` (KAIROS-T-0431, an organization admin).
    pub async fn set_org_read_token(
        &self,
        host: &str,
        token: &crate::types_auth::Secret,
    ) -> Result<crate::types_code_index::OrgReadToken, Error> {
        self.put_ok(
            &format!("/api/org/read-tokens/{host}"),
            &crate::types_code_index::SetOrgReadTokenRequest {
                token: token.clone(),
            },
        )
        .await
    }

    /// `DELETE /api/org/read-tokens/{host}` — remove the read token of the
    /// organization for `host` (KAIROS-T-0431, an organization admin). The
    /// answer is the list that stays.
    pub async fn remove_org_read_token(
        &self,
        host: &str,
    ) -> Result<Vec<crate::types_code_index::OrgReadToken>, Error> {
        self.delete(&format!("/api/org/read-tokens/{host}")).await
    }

    /// `POST /api/code-index/query-vector` — the vector of the text of a
    /// code search, made by the model `model` of the vectors of an index
    /// (KAIROS-T-0360). The CLI asks for it when it has no local model of
    /// the index.
    pub async fn query_vector(
        &self,
        model: &str,
        text: &str,
    ) -> Result<crate::types_code_index::QueryVector, Error> {
        self.post_ok(
            "/api/code-index/query-vector",
            &crate::types_code_index::QueryVectorRequest {
                model: model.to_string(),
                text: text.to_string(),
            },
        )
        .await
    }

    /// `POST /api/repositories/{slug}/code-indexes/rebuild` — ask the
    /// builder of the server for a full build of the code index
    /// (KAIROS-T-0332). The answer is 202 with the `running` run.
    pub async fn rebuild_code_index(
        &self,
        reference: &str,
    ) -> Result<crate::types_code_index::CodeIndexBuild, Error> {
        let path = format!("/api/repositories/{reference}/code-indexes/rebuild");
        self.execute(format!("POST {path}"), 202, self.http.post(self.url(&path)))
            .await
    }

    /// `PUT /api/repositories/{slug}/code-indexes/{commit}` — send the
    /// index file of a commit (the bytes of a `kairos-index` SQLite file).
    /// The answer is 201 for a new commit and 200 for a commit that had an
    /// index; both are a success here.
    pub async fn upload_code_index(
        &self,
        reference: &str,
        commit: &str,
        r#ref: Option<&str>,
        index_file: Vec<u8>,
    ) -> Result<crate::types_code_index::UploadedCodeIndex, Error> {
        let path = format!("/api/repositories/{reference}/code-indexes/{commit}");
        let query = crate::types_code_index::UploadCodeIndexQuery {
            r#ref: r#ref.map(str::to_string),
        };
        let builder = self
            .http
            .put(self.url(&path))
            .query(&query)
            .header(reqwest::header::CONTENT_TYPE, "application/vnd.sqlite3")
            .body(index_file);
        let response = self.authorize(builder).await?.send().await?;
        let status = response.status().as_u16();
        let body = response.text().await?;
        if status == 200 || status == 201 {
            return serde_json::from_str(&body).map_err(|source| Error::Decode {
                context: format!("PUT {path}"),
                source,
            });
        }
        match serde_json::from_str::<ErrorEnvelope>(&body) {
            Ok(envelope) => Err(Error::from_envelope(status, envelope)),
            Err(_) => Err(Error::UnexpectedResponse { status, body }),
        }
    }

    /// `GET /api/repositories/{slug}/code-indexes/{commit}` — the index
    /// file of a commit, with the summaries that it uses.
    pub async fn download_code_index(
        &self,
        reference: &str,
        commit: &str,
    ) -> Result<Vec<u8>, Error> {
        let path = format!("/api/repositories/{reference}/code-indexes/{commit}");
        let builder = self.http.get(self.url(&path));
        let response = self.authorize(builder).await?.send().await?;
        let status = response.status().as_u16();
        if status == 200 {
            return Ok(response.bytes().await?.to_vec());
        }
        let body = response.text().await?;
        match serde_json::from_str::<ErrorEnvelope>(&body) {
            Ok(envelope) => Err(Error::from_envelope(status, envelope)),
            Err(_) => Err(Error::UnexpectedResponse { status, body }),
        }
    }

    /// `GET /api/repositories/{slug}/code-indexes/nearest?commit=` — the
    /// nearest indexed commit at or below `commit`.
    pub async fn nearest_code_index(
        &self,
        reference: &str,
        commit: &str,
    ) -> Result<crate::types_code_index::NearestCodeIndex, Error> {
        self.get_query(
            &format!("/api/repositories/{reference}/code-indexes/nearest"),
            &crate::types_code_index::NearestCodeIndexQuery {
                commit: commit.to_string(),
            },
        )
        .await
    }

    /// `DELETE /api/forge-connections/{id}` (org admin).
    pub async fn delete_forge_connection(&self, id: &str) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!("/api/forge-connections/{id}")).await
    }

    /// `POST /api/forge-connections/{id}/rotate` — mints a new connection
    /// id (and therefore a new URL and secret) for the same repository.
    pub async fn rotate_forge_connection(
        &self,
        id: &str,
    ) -> Result<crate::types_forge::CreatedForgeConnection, Error> {
        self.post_empty(&format!("/api/forge-connections/{id}/rotate"))
            .await
    }

    /// `GET /api/{family}/{short_code}/graph?depth=N` — the focal
    /// subgraph: nodes AND typed directed edges (KAIROS-T-0088). `None`
    /// depth takes the server default (2).
    pub async fn get_item_graph(
        &self,
        kind: EntityKind,
        short_code: &str,
        depth: Option<u32>,
    ) -> Result<crate::types_graph::GraphResponse, Error> {
        let path = match depth {
            Some(depth) => format!("/api/{kind}/{short_code}/graph?depth={depth}"),
            None => format!("/api/{kind}/{short_code}/graph"),
        };
        self.get(&path).await
    }

    /// `POST /api/relationships`. The link rule applies
    /// (COLLIERY-T-0228): the caller may edit the item at either end. A
    /// `blocks` edge needs the target (KAIROS-T-0569). A 202 (a proposal)
    /// is an [`Error::UnexpectedResponse`] here: use
    /// [`Self::link_or_propose`] to read it.
    pub async fn create_relationship(
        &self,
        request: &CreateRelationshipRequest,
    ) -> Result<Relationship, Error> {
        self.post_created("/api/relationships", request).await
    }

    /// `POST /api/relationships`, with the two answers (KAIROS-T-0569):
    /// 201 the edge, 202 a proposal of a `blocks` edge from a caller who
    /// can edit the source only.
    pub async fn link_or_propose(
        &self,
        request: &CreateRelationshipRequest,
    ) -> Result<RelationshipOutcome, Error> {
        let path = "/api/relationships";
        let context = format!("POST {path}");
        let builder = self
            .authorize(self.http.post(self.url(path)).json(request))
            .await?;
        let response = builder.send().await?;
        let status = response.status().as_u16();
        let body = response.text().await?;
        let decode = |source| Error::Decode {
            context: context.clone(),
            source,
        };
        match status {
            201 => serde_json::from_str::<Relationship>(&body)
                .map(RelationshipOutcome::Linked)
                .map_err(decode),
            202 => serde_json::from_str::<ProposedRelationship>(&body)
                .map(RelationshipOutcome::Proposed)
                .map_err(decode),
            _ => match serde_json::from_str::<ErrorEnvelope>(&body) {
                Ok(envelope) if !(200..300).contains(&status) => {
                    Err(Error::from_envelope(status, envelope))
                }
                _ => Err(Error::UnexpectedResponse { status, body }),
            },
        }
    }

    /// `PUT /api/{family}/{short_code}/parent` (KAIROS-T-0572): set the
    /// parent of an initiative or a task, or remove it with `None`. The new
    /// edge replaces the old one in one transaction. The link rule applies
    /// to each edge that changes.
    pub async fn set_parent(
        &self,
        kind: EntityKind,
        short_code: &str,
        parent: Option<&str>,
    ) -> Result<crate::types_meta::SetParentResponse, Error> {
        self.put_ok(
            &format!("/api/{kind}/{short_code}/parent"),
            &crate::types_meta::SetParentRequest {
                parent: parent.map(str::to_string),
            },
        )
        .await
    }

    /// `DELETE /api/relationships/{id}`. The link rule applies, as for
    /// the create. For the `supports` edge of a document the caller must
    /// be able to edit the document (COLLIERY-T-0235).
    pub async fn delete_relationship(
        &self,
        relationship_id: &str,
    ) -> Result<DeletedResponse, Error> {
        self.delete(&format!("/api/relationships/{relationship_id}"))
            .await
    }

    // -- item metadata (KAIROS-A-0003) --------------------------------------------

    /// `GET /api/{family}/{short_code}/metadata`.
    pub async fn metadata(
        &self,
        kind: EntityKind,
        short_code: &str,
    ) -> Result<ItemMetadataResponse, Error> {
        self.get(&format!("/api/{kind}/{short_code}/metadata"))
            .await
    }

    /// `PATCH /api/{family}/{short_code}/metadata` — typed upsert; `null`
    /// clears a value.
    pub async fn update_metadata(
        &self,
        kind: EntityKind,
        short_code: &str,
        request: &UpdateMetadataRequest,
    ) -> Result<ItemMetadataResponse, Error> {
        self.patch(&format!("/api/{kind}/{short_code}/metadata"), request)
            .await
    }

    // -- metadata definitions ------------------------------------------------------

    // -- team pages + announcements (KAIROS-T-0083) -------------------------

    /// `GET /api/teams/by-slug/{slug}`.
    pub async fn get_team_by_slug(&self, slug: &str) -> Result<Team, Error> {
        self.get(&format!("/api/teams/by-slug/{slug}")).await
    }

    /// `GET /api/teams/{id}/pages` — the team's live page tree as a flat
    /// list (nest by parent_id).
    pub async fn list_team_pages(
        &self,
        team_id: &str,
    ) -> Result<Vec<crate::types_team_pages::TeamPageSummary>, Error> {
        self.get(&format!("/api/teams/{team_id}/pages")).await
    }

    /// `GET /api/teams/{id}/pages/{page_id}`.
    pub async fn get_team_page(&self, team_id: &str, page_id: &str) -> Result<TeamPage, Error> {
        self.get(&format!("/api/teams/{team_id}/pages/{page_id}"))
            .await
    }

    /// `POST /api/teams/{id}/pages` (team member or org admin).
    pub async fn create_team_page(
        &self,
        team_id: &str,
        request: &CreateTeamPageRequest,
    ) -> Result<TeamPage, Error> {
        self.post_created(&format!("/api/teams/{team_id}/pages"), request)
            .await
    }

    /// `PATCH /api/teams/{id}/pages/{page_id}` — content edit
    /// (version-checked) or rename/move.
    pub async fn update_team_page(
        &self,
        team_id: &str,
        page_id: &str,
        request: &UpdateTeamPageRequest,
    ) -> Result<TeamPage, Error> {
        self.patch(&format!("/api/teams/{team_id}/pages/{page_id}"), request)
            .await
    }

    /// `DELETE /api/teams/{id}/pages/{page_id}` (soft; folders must be
    /// empty).
    pub async fn delete_team_page(
        &self,
        team_id: &str,
        page_id: &str,
    ) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!("/api/teams/{team_id}/pages/{page_id}"))
            .await
    }

    /// `GET /api/teams/{id}/tree-items` — the documents and ADRs of a team
    /// with their places in its tree (KAIROS-T-0437).
    pub async fn list_team_tree_items(
        &self,
        team_id: &str,
    ) -> Result<Vec<crate::types_team_pages::TeamTreeItem>, Error> {
        self.get(&format!("/api/teams/{team_id}/tree-items")).await
    }

    /// `PUT /api/teams/{id}/tree-items/{item}` — put a document or an ADR of
    /// the team in a folder (team member or org admin).
    pub async fn place_team_tree_item(
        &self,
        team_id: &str,
        item: &str,
        request: &crate::types_team_pages::PlaceTeamItemRequest,
    ) -> Result<crate::types_team_pages::TeamTreeItem, Error> {
        self.put_ok(&format!("/api/teams/{team_id}/tree-items/{item}"), request)
            .await
    }

    /// `GET /api/teams/{id}/archived-pages` — the archived nodes, newest
    /// archive first (KAIROS-T-0439).
    pub async fn list_archived_team_pages(
        &self,
        team_id: &str,
    ) -> Result<Vec<crate::types_team_pages::ArchivedTeamPage>, Error> {
        self.get(&format!("/api/teams/{team_id}/archived-pages"))
            .await
    }

    /// `POST /api/teams/{id}/pages/{page_id}/restore` — restore a node and
    /// what was archived with it (team member or org admin).
    pub async fn restore_team_page(&self, team_id: &str, page_id: &str) -> Result<TeamPage, Error> {
        self.post_empty(&format!("/api/teams/{team_id}/pages/{page_id}/restore"))
            .await
    }

    /// `GET /api/teams/{id}/pages/{page_id}/history` — one page of the
    /// saved versions, newest first, without content.
    pub async fn team_page_history(
        &self,
        team_id: &str,
        page_id: &str,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<ListEnvelope<crate::types_team_pages::TeamPageVersionSummary>, Error> {
        self.get_query(
            &format!("/api/teams/{team_id}/pages/{page_id}/history"),
            &crate::types_team_pages::TeamPageHistoryQuery {
                version: None,
                limit,
                offset,
            },
        )
        .await
    }

    /// `GET /api/teams/{id}/pages/{page_id}/history?version=N` — one saved
    /// version in full.
    pub async fn team_page_version(
        &self,
        team_id: &str,
        page_id: &str,
        version: i32,
    ) -> Result<crate::types_team_pages::TeamPageVersion, Error> {
        self.get_query(
            &format!("/api/teams/{team_id}/pages/{page_id}/history"),
            &crate::types_team_pages::TeamPageHistoryQuery {
                version: Some(version),
                limit: None,
                offset: None,
            },
        )
        .await
    }

    /// `GET /api/teams/{id}/work-documents` — live documents supporting
    /// the team's tasks or items on its delivery board (KAIROS-T-0084).
    pub async fn list_team_work_documents(
        &self,
        team_id: &str,
    ) -> Result<Vec<TeamWorkDocument>, Error> {
        self.get(&format!("/api/teams/{team_id}/work-documents"))
            .await
    }

    /// `GET /api/teams/{id}/announcements` — one page, pinned first,
    /// newest first.
    pub async fn list_team_announcements(
        &self,
        team_id: &str,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<ListEnvelope<TeamAnnouncement>, Error> {
        self.get_query(
            &format!("/api/teams/{team_id}/announcements"),
            &Pagination { limit, offset },
        )
        .await
    }

    /// `POST /api/teams/{id}/announcements` (team member or org admin;
    /// append-only — there is no edit).
    pub async fn create_team_announcement(
        &self,
        team_id: &str,
        request: &CreateTeamAnnouncementRequest,
    ) -> Result<TeamAnnouncement, Error> {
        self.post_created(&format!("/api/teams/{team_id}/announcements"), request)
            .await
    }

    /// `DELETE /api/teams/{id}/announcements/{announcement_id}` (author
    /// or org admin).
    pub async fn delete_team_announcement(
        &self,
        team_id: &str,
        announcement_id: &str,
    ) -> Result<OrgDeleteResponse, Error> {
        self.delete(&format!(
            "/api/teams/{team_id}/announcements/{announcement_id}"
        ))
        .await
    }

    /// `GET /api/metadata-definitions`.
    pub async fn list_metadata_definitions(
        &self,
        page: Pagination,
    ) -> Result<ListEnvelope<MetadataDefinition>, Error> {
        self.get_query("/api/metadata-definitions", &page).await
    }

    /// `GET /api/metadata-definitions?entity_type=…` — the catalog in
    /// scope for one entity type (KAIROS-T-0078): unscoped definitions
    /// plus those whose scopes include it.
    pub async fn list_metadata_definitions_for(
        &self,
        page: Pagination,
        entity_type: Option<&str>,
    ) -> Result<ListEnvelope<MetadataDefinition>, Error> {
        #[derive(serde::Serialize)]
        struct DefinitionQuery<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            offset: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            entity_type: Option<&'a str>,
        }
        self.get_query(
            "/api/metadata-definitions",
            &DefinitionQuery {
                limit: page.limit,
                offset: page.offset,
                entity_type,
            },
        )
        .await
    }

    /// `GET /api/metadata-definitions/{id}`. `id` is the slug or the id
    /// (KAIROS-T-0644), as on `update` and `delete`.
    pub async fn get_metadata_definition(&self, id: &str) -> Result<MetadataDefinition, Error> {
        self.get(&format!("/api/metadata-definitions/{id}")).await
    }

    /// `POST /api/metadata-definitions` (org admin).
    pub async fn create_metadata_definition(
        &self,
        request: &CreateMetadataDefinitionRequest,
    ) -> Result<MetadataDefinition, Error> {
        self.post_created("/api/metadata-definitions", request)
            .await
    }

    /// `PATCH /api/metadata-definitions/{id}` (org admin).
    pub async fn update_metadata_definition(
        &self,
        id: &str,
        request: &UpdateMetadataDefinitionRequest,
    ) -> Result<MetadataDefinition, Error> {
        self.patch(&format!("/api/metadata-definitions/{id}"), request)
            .await
    }

    /// `DELETE /api/metadata-definitions/{id}` (org admin; 409
    /// `DEFINITION_IN_USE` when referenced).
    pub async fn delete_metadata_definition(&self, id: &str) -> Result<DeletedResponse, Error> {
        self.delete(&format!("/api/metadata-definitions/{id}"))
            .await
    }

    // -- templates -------------------------------------------------------------------

    /// `GET /api/templates`.
    pub async fn list_templates(&self, page: Pagination) -> Result<ListEnvelope<Template>, Error> {
        self.get_query("/api/templates", &page).await
    }

    /// `GET /api/templates/{id}` — template + metadata associations. `id`
    /// is the slug or the id (KAIROS-T-0644), as on `update` and `delete`.
    pub async fn get_template(&self, id: &str) -> Result<TemplateDetail, Error> {
        self.get(&format!("/api/templates/{id}")).await
    }

    /// `POST /api/templates` (org admin).
    pub async fn create_template(
        &self,
        request: &CreateTemplateRequest,
    ) -> Result<TemplateDetail, Error> {
        self.post_created("/api/templates", request).await
    }

    /// `PATCH /api/templates/{id}` (org admin).
    pub async fn update_template(
        &self,
        id: &str,
        request: &UpdateTemplateRequest,
    ) -> Result<TemplateDetail, Error> {
        self.patch(&format!("/api/templates/{id}"), request).await
    }

    /// `DELETE /api/templates/{id}` (org admin; hard delete).
    pub async fn delete_template(&self, id: &str) -> Result<DeletedResponse, Error> {
        self.delete(&format!("/api/templates/{id}")).await
    }

    // -- content history (KAIROS-A-0004) ------------------------------------------------

    /// `GET /api/{family}/{short_code}/history` — the version list,
    /// newest first.
    pub async fn history(
        &self,
        kind: EntityKind,
        short_code: &str,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<ListEnvelope<HistoryVersion>, Error> {
        self.get_query(
            &format!("/api/{kind}/{short_code}/history"),
            &HistoryQuery {
                version: None,
                limit,
                offset,
            },
        )
        .await
    }

    /// `GET /api/{family}/{short_code}/history?version=N` — one full
    /// snapshot.
    pub async fn history_snapshot(
        &self,
        kind: EntityKind,
        short_code: &str,
        version: i32,
    ) -> Result<HistorySnapshot, Error> {
        self.get_query(
            &format!("/api/{kind}/{short_code}/history"),
            &HistoryQuery {
                version: Some(version),
                limit: None,
                offset: None,
            },
        )
        .await
    }

    /// `GET /api/{family}/{short_code}/activity` — the activity of the
    /// item, newest first, with names (KAIROS-T-0557).
    pub async fn item_activity(
        &self,
        kind: EntityKind,
        short_code: &str,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<ListEnvelope<ItemActivityEntry>, Error> {
        self.get_query(
            &format!("/api/{kind}/{short_code}/activity"),
            &ItemActivityQuery { limit, offset },
        )
        .await
    }

    // -- notes (KAIROS-T-0367) ---------------------------------------------------------------

    /// `POST /api/{family}/{short_code}/notes` — append a note at the end
    /// of the item's content (no version, no anchor).
    pub async fn append_note(
        &self,
        kind: EntityKind,
        short_code: &str,
        text: &str,
    ) -> Result<AppendNoteResponse, Error> {
        self.post_ok(
            &format!("/api/{kind}/{short_code}/notes"),
            &AppendNoteRequest {
                text: text.to_string(),
            },
        )
        .await
    }

    // -- pre-delete cascade preview (KAIROS-T-0051) -----------------------------------------

    /// `GET /api/{family}/{short_code}/cascade-preview` — the AUTHORITATIVE
    /// KAIROS-A-0001 descendant set a soft-delete WOULD cascade to, computed
    /// without deleting. `cascaded_short_codes` matches the
    /// [`DeleteResponse`] a subsequent delete would return.
    pub async fn cascade_preview(
        &self,
        kind: EntityKind,
        short_code: &str,
    ) -> Result<CascadePreviewResponse, Error> {
        self.get(&format!("/api/{kind}/{short_code}/cascade-preview"))
            .await
    }

    // -- activity log ----------------------------------------------------------------------

    /// `GET /api/activity` with combinable filters (S-0005).
    pub async fn activity(
        &self,
        query: &ActivityQuery,
    ) -> Result<ListEnvelope<ActivityEntry>, Error> {
        self.get_query("/api/activity", query).await
    }

    // -- unified search (KAIROS-A-0007) ------------------------------------------------------

    /// `POST /api/search` — full-text + filter + traverse composition.
    pub async fn search(&self, request: &SearchRequest) -> Result<SearchResponse, Error> {
        self.post_ok("/api/search", request).await
    }

    /// `POST /api/search` with a body as it is (KAIROS-T-0593): the server
    /// judges each field, so a field that this client does not know still
    /// goes to a server that knows it.
    pub async fn search_raw(&self, body: &serde_json::Value) -> Result<SearchResponse, Error> {
        self.post_ok("/api/search", body).await
    }

    // -- identity ------------------------------------------------------------------------------

    /// `GET /api/whoami` — the resolved user/tenant/teams identity probe.
    pub async fn whoami(&self) -> Result<WhoamiResponse, Error> {
        self.get("/api/whoami").await
    }

    // -- local accounts (KAIROS-I-0018, COLLIERY-T-0213) ---------------------------------------

    /// `GET /api/config` — how this deployment lets people log in. Public:
    /// the request carries no bearer.
    pub async fn deployment_config(&self) -> Result<DeploymentConfig, Error> {
        let path = "/api/config";
        let builder = self.without_credential(self.http.get(self.url(path)));
        Self::send_and_decode(format!("GET {path}"), 200, builder).await
    }

    /// `POST /api/login` — exchange an email and a password for a session
    /// bearer. Public: the request carries no bearer.
    ///
    /// The route exists only on a deployment with local accounts on. On any
    /// other deployment the result is [`Error::NotFound`] or
    /// [`Error::UnexpectedResponse`].
    ///
    /// No error from this method contains the response body. An unexpected
    /// 2xx status could carry the token in its body, and an error is a thing
    /// that callers print.
    pub async fn login(&self, request: &LoginRequest) -> Result<LoginResponse, Error> {
        let path = "/api/login";
        let builder = self.without_credential(self.http.post(self.url(path)).json(request));
        match Self::send_and_decode(format!("POST {path}"), 200, builder).await {
            Err(Error::UnexpectedResponse { status, .. }) => Err(Error::UnexpectedResponse {
                status,
                body: "(body withheld: a login response can hold a credential)".to_string(),
            }),
            other => other,
        }
    }

    /// `POST /api/logout` — end the session of this client's bearer. The
    /// server answers 204 with no body, also for a session that is already
    /// over.
    pub async fn logout(&self) -> Result<(), Error> {
        let path = "/api/logout";
        let response = self
            .authorize(self.http.post(self.url(path)))
            .await?
            .send()
            .await?;
        let status = response.status().as_u16();
        if status == 204 {
            return Ok(());
        }
        let body = response.text().await?;
        match serde_json::from_str::<ErrorEnvelope>(&body) {
            Ok(envelope) if !(200..300).contains(&status) => {
                Err(Error::from_envelope(status, envelope))
            }
            _ => Err(Error::UnexpectedResponse { status, body }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::State;
    use axum::http::HeaderMap;
    use axum::routing::get;

    /// A server on an ephemeral port whose `/api/config` answers after
    /// `delay` with the `Accept-Encoding` of the request as the issuer.
    async fn serve(delay: Duration) -> String {
        async fn config(State(delay): State<Duration>, headers: HeaderMap) -> axum::Json<Value> {
            tokio::time::sleep(delay).await;
            let accept = headers
                .get("accept-encoding")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            axum::Json(serde_json::json!({"issuer": accept, "local_auth": false}))
        }
        let app = Router::new()
            .route("/api/config", get(config))
            .with_state(delay);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        format!("http://{address}")
    }

    /// KAIROS-T-0607: `KAIROS_TIMEOUT` is a positive integer of seconds, or
    /// nothing. Anything else is refused with the name of the variable.
    #[test]
    fn the_timeout_setting_is_a_positive_integer_of_seconds() {
        assert_eq!(parse_timeout(None).expect("unset"), None);
        assert_eq!(parse_timeout(Some("")).expect("empty"), None);
        assert_eq!(parse_timeout(Some("  ")).expect("blank"), None);
        assert_eq!(
            parse_timeout(Some("120")).expect("seconds"),
            Some(Duration::from_secs(120))
        );
        assert_eq!(
            parse_timeout(Some(" 7 ")).expect("trimmed"),
            Some(Duration::from_secs(7))
        );
        for bad in ["0", "-1", "abc", "1.5", "60s"] {
            let err = parse_timeout(Some(bad)).expect_err(bad);
            match &err {
                Error::Setting { variable, message } => {
                    assert_eq!(*variable, "KAIROS_TIMEOUT");
                    assert!(message.contains("KAIROS_TIMEOUT"), "{message}");
                    assert!(message.contains(bad), "{message}");
                }
                other => panic!("{bad}: expected Setting, got {other}"),
            }
            assert!(err.to_string().contains("KAIROS_TIMEOUT"), "{err}");
        }
    }

    /// KAIROS-T-0607: a new client has the default timeouts; the builders
    /// change one and keep the other.
    #[test]
    fn a_client_has_the_default_timeouts() {
        let client = KairosClient::anonymous("http://kairos.test/");
        assert_eq!(client.timeout(), Duration::from_secs(60));
        assert_eq!(client.connect_timeout(), Duration::from_secs(5));
        assert_eq!(client.base_url(), "http://kairos.test");

        let client = client.with_timeout(Duration::from_secs(120));
        assert_eq!(client.timeout(), Duration::from_secs(120));
        assert_eq!(client.connect_timeout(), Duration::from_secs(5));

        let client = client.with_connect_timeout(Duration::from_secs(1));
        assert_eq!(client.timeout(), Duration::from_secs(120));
        assert_eq!(client.connect_timeout(), Duration::from_secs(1));
    }

    /// KAIROS-T-0607: every request asks for a compressed answer.
    #[tokio::test]
    async fn the_client_accepts_gzip_and_brotli() {
        let base_url = serve(Duration::ZERO).await;
        let config = KairosClient::anonymous(base_url)
            .deployment_config()
            .await
            .expect("config");
        let accept = config.issuer.expect("the Accept-Encoding of the request");
        assert!(accept.contains("gzip"), "{accept}");
        assert!(accept.contains("br"), "{accept}");
    }

    /// KAIROS-T-0607: a server that does not answer in time is a transport
    /// error, not a hang.
    #[tokio::test]
    async fn a_slow_server_is_a_timeout() {
        let base_url = serve(Duration::from_secs(5)).await;
        let err = KairosClient::anonymous(base_url)
            .with_timeout(Duration::from_millis(200))
            .deployment_config()
            .await
            .expect_err("the request times out");
        match err {
            Error::Transport(source) => assert!(source.is_timeout(), "{source}"),
            other => panic!("expected Transport, got {other}"),
        }
    }
}
