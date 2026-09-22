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

use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::Error;
use crate::types::{
    Adr, CascadePreviewResponse, CreateAdrRequest, CreateDocumentRequest, CreateInitiativeRequest,
    CreateStrategyRequest, CreateTaskRequest, DeleteResponse, Document, ErrorEnvelope, Initiative,
    ListEnvelope, Pagination, SetLifecycleRequest, SetWorkClassRequest, Strategy, Task,
    TransitionRequest, UpdateContentRequest,
};
use crate::types_meta::{
    ActivityEntry, ActivityQuery, ChildrenProgressResponse, CreateMetadataDefinitionRequest,
    CreateRelationshipRequest, CreateTemplateRequest, DeletedResponse, HistoryQuery,
    HistorySnapshot, HistoryVersion, ItemMetadataResponse, ItemRelationshipsResponse,
    MetadataDefinition, Relationship, Template, TemplateDetail, UpdateMetadataDefinitionRequest,
    UpdateMetadataRequest, UpdateTemplateRequest,
};
use crate::types_org::{
    AddBoardMemberRequest, AddOrgMemberRequest, AddStreamTeamRequest, AddTeamMemberRequest, Board,
    BoardColumn, BoardDetail, BoardItemsResponse, BoardMember, BoardTransition, CreateBoardRequest,
    CreateColumnRequest, CreateStreamRequest, CreateTeamRequest, CreateTenantRequest,
    CreateTransitionRequest, DeliveryStream, OrgDeleteResponse, OrgMember,
    RemoveBoardMemberResponse, RemoveOrgMemberResponse, ReplaceCapabilitiesRequest, Team,
    TeamMember, TenantCreatedResponse, TenantDeletedResponse, TenantSummary, UpdateBoardRequest,
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
#[derive(Debug, Clone)]
pub struct StaticToken(pub String);

impl TokenProvider for StaticToken {
    fn bearer_token(&self) -> Pin<Box<dyn Future<Output = Result<String, Error>> + Send + '_>> {
        let token = self.0.clone();
        Box::pin(async move { Ok(token) })
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

/// The typed Kairos API client. Cheap to clone.
#[derive(Clone)]
pub struct KairosClient {
    pub(crate) http: reqwest::Client,
    /// Base URL without a trailing slash (`http://host:port`).
    pub(crate) base_url: String,
    pub(crate) token: Arc<dyn TokenProvider>,
    /// `X-Tenant` header value (dev/test tenant resolution fallback).
    pub(crate) tenant: Option<String>,
}

impl fmt::Debug for KairosClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KairosClient")
            .field("base_url", &self.base_url)
            .field("tenant", &self.tenant)
            .finish_non_exhaustive()
    }
}

impl KairosClient {
    /// A client against `base_url` drawing tokens from `token`.
    pub fn new(base_url: impl Into<String>, token: Arc<dyn TokenProvider>) -> Self {
        let mut base_url = base_url.into();
        while base_url.ends_with('/') {
            base_url.pop();
        }
        Self {
            http: reqwest::Client::new(),
            base_url,
            token,
            tenant: None,
        }
    }

    /// A client with a fixed bearer token (integration tests).
    pub fn with_static_token(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self::new(base_url, Arc::new(StaticToken(token.into())))
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
        let response = self.authorize(builder).await?.send().await?;
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
     $list:ident, $get:ident, $create:ident, $update:ident, $delete:ident) => {
        #[doc = concat!("`GET /api/", $family, "` — list (tenant-open read).")]
        pub async fn $list(&self, page: Pagination) -> Result<ListEnvelope<$dto>, Error> {
            self.get_query(concat!("/api/", $family), &page).await
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
        delete_strategy
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
        delete_initiative
    );
    entity_transition!("initiatives", Initiative, transition_initiative);

    entity_family!(
        "tasks",
        Task,
        CreateTaskRequest,
        list_tasks,
        get_task,
        create_task,
        update_task,
        delete_task
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

    /// `PUT /api/tasks/{short_code}/repository` — bind the task to a
    /// repository (slug or UUID) or clear it with `None` (KAIROS-T-0104).
    /// The repository must belong to the team whose delivery board the
    /// task sits on; requires `manage_tasks` on that board.
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
    /// entity types are unaffected.
    pub async fn board_items_for_repository(
        &self,
        board_id: &str,
        repository: &str,
    ) -> Result<BoardItemsResponse, Error> {
        // Slugs are `[a-z0-9-]` and UUIDs are hex, so no encoding is needed
        // for valid input; anything else is rejected server-side anyway.
        self.get(&format!(
            "/api/boards/{board_id}/items?repository={repository}"
        ))
        .await
    }

    entity_family!(
        "documents",
        Document,
        CreateDocumentRequest,
        list_documents,
        get_document,
        create_document,
        update_document,
        delete_document
    );

    entity_family!(
        "adrs",
        Adr,
        CreateAdrRequest,
        list_adrs,
        get_adr,
        create_adr,
        update_adr,
        delete_adr
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

    /// `GET /api/boards/{id}/items` — every live item grouped by column.
    pub async fn board_items(&self, board_id: &str) -> Result<BoardItemsResponse, Error> {
        self.get(&format!("/api/boards/{board_id}/items")).await
    }

    /// `GET /api/boards/{id}/columns`.
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

    /// `GET /api/boards/{id}/transitions`.
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

    /// `PATCH /api/forge-connections/{id}` — team attribution only.
    pub async fn update_forge_connection(
        &self,
        id: &str,
        request: &crate::types_forge::UpdateForgeConnectionRequest,
    ) -> Result<crate::types_forge::ForgeConnection, Error> {
        self.patch(&format!("/api/forge-connections/{id}"), request)
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
        self.post_ok(
            &format!("/api/forge-connections/{id}/rotate"),
            &serde_json::json!({}),
        )
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

    /// `POST /api/relationships` (org admin, KAIROS-A-0006).
    pub async fn create_relationship(
        &self,
        request: &CreateRelationshipRequest,
    ) -> Result<Relationship, Error> {
        self.post_created("/api/relationships", request).await
    }

    /// `DELETE /api/relationships/{id}` (org admin).
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
    pub async fn list_team_pages(&self, team_id: &str) -> Result<Vec<TeamPage>, Error> {
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

    /// `GET /api/teams/{id}/announcements` — pinned first, newest first.
    /// `GET /api/teams/{id}/work-documents` — live documents supporting
    /// the team's tasks or items on its delivery board (KAIROS-T-0084).
    pub async fn list_team_work_documents(
        &self,
        team_id: &str,
    ) -> Result<Vec<TeamWorkDocument>, Error> {
        self.get(&format!("/api/teams/{team_id}/work-documents"))
            .await
    }

    pub async fn list_team_announcements(
        &self,
        team_id: &str,
    ) -> Result<Vec<TeamAnnouncement>, Error> {
        self.get(&format!("/api/teams/{team_id}/announcements"))
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

    /// `GET /api/metadata-definitions/{id}`.
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

    /// `GET /api/templates/{id}` — template + metadata associations.
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

    // -- identity ------------------------------------------------------------------------------

    /// `GET /api/whoami` — the resolved user/tenant/teams identity probe.
    pub async fn whoami(&self) -> Result<WhoamiResponse, Error> {
        self.get("/api/whoami").await
    }
}
