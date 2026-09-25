//! `/scim/v2/Users` (RFC 7644 §3): the tenant's SCIM User resource set,
//! which IS its `organization_members` rows (see [`super`] module docs for
//! the identity-join and deprovision contracts).

use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_db::models::enums::{ActivityAction, OrgRole};
use kairos_db::models::{NewOrganizationMember, NewUser, OrganizationMember, User};
use serde_json::{Value, json};
use uuid::Uuid;

use super::auth::ScimContext;
use super::error::{ScimError, scim_response};
use super::{
    ListParams, PATCH_URN, USER_URN, admin_count, last_admin_error, list_response, load_user,
    log_scim_activity, membership_of, no_content, parse_eq_filter, parse_json_body,
    parse_resource_id, run_scim, scim_transaction,
};
use crate::app::AppState;

// ---------------------------------------------------------------------------
// Resource rendering
// ---------------------------------------------------------------------------

/// Render one `public.users` row as a SCIM User resource.
///
/// `userName` comes from `users.user_name` and `externalId` from
/// `users.external_id` — two columns since KAIROS-T-0184, because they are two
/// things: `externalId` is the OIDC `sub` logins join on, `userName` is the login
/// identifier the IdP uses, commonly an email. Existing rows were backfilled
/// `user_name = external_id`, so a deployment that never sent a distinct
/// `userName` sees exactly what it saw before.
fn user_resource(user: &User, active: bool) -> Value {
    json!({
        "schemas": [USER_URN],
        "id": user.id.to_string(),
        "userName": user.user_name,
        "externalId": user.external_id,
        "displayName": user.display_name,
        "emails": [{ "value": user.email, "primary": true }],
        "active": active,
        "meta": {
            "resourceType": "User",
            "created": user.created_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            "lastModified": user.updated_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            "location": format!("/scim/v2/Users/{}", user.id),
        },
    })
}

// ---------------------------------------------------------------------------
// Payload parsing
// ---------------------------------------------------------------------------

/// The inbound attribute subset of a POST/PUT User payload.
struct UserPayload {
    user_name: String,
    external_id: Option<String>,
    display_name: Option<String>,
    /// First primary (else first) email value; falls back to `userName`
    /// when it contains `@`.
    email: Option<String>,
    /// `active` (defaults true); `false` in a PUT deprovisions.
    active: bool,
}

/// A SCIM boolean: JSON bool, or Entra's `"True"`/`"False"` strings.
fn parse_scim_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::String(s) if s.eq_ignore_ascii_case("true") => Some(true),
        Value::String(s) if s.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

/// The first usable email value: primary first, then first entry; entries
/// may be `{"value": "..."}` objects or plain strings.
fn first_email(body: &Value) -> Option<String> {
    let entries = body.get("emails")?.as_array()?;
    let value_of = |entry: &Value| -> Option<String> {
        match entry {
            Value::String(s) => Some(s.clone()),
            other => other.get("value")?.as_str().map(str::to_string),
        }
    };
    entries
        .iter()
        .find(|e| e.get("primary").and_then(Value::as_bool) == Some(true))
        .and_then(value_of)
        .or_else(|| entries.first().and_then(value_of))
}

fn parse_user_payload(body: &Value) -> Result<UserPayload, ScimError> {
    let user_name = body
        .get("userName")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ScimError::invalid_value("userName is required"))?
        .to_string();
    let external_id = body
        .get("externalId")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);
    let display_name = body
        .get("displayName")
        .and_then(Value::as_str)
        .or_else(|| {
            body.get("name")
                .and_then(|n| n.get("formatted"))
                .and_then(Value::as_str)
        })
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);
    let email = first_email(body).or_else(|| user_name.contains('@').then(|| user_name.clone()));
    let active = match body.get("active") {
        None | Some(Value::Null) => true,
        Some(v) => parse_scim_bool(v)
            .ok_or_else(|| ScimError::invalid_value("active must be a boolean"))?,
    };
    Ok(UserPayload {
        user_name,
        external_id,
        display_name,
        email,
        active,
    })
}

// ---------------------------------------------------------------------------
// Join + lifecycle plumbing
// ---------------------------------------------------------------------------

/// The identity-join contract (module docs): externalId → external_id, then
/// userName → user_name, then email fallback, else create.
///
/// The `userName` arm searches `user_name` rather than `external_id` since
/// KAIROS-T-0184. Against an IdP whose `userName` is a login email and whose
/// `sub` is opaque, the old arm could never match, so reconciliation concluded
/// the user was absent and re-created them.
fn resolve_or_create_user(
    conn: &mut PgConnection,
    payload: &UserPayload,
) -> Result<User, ScimError> {
    use kairos_db::schema::users;

    // externalId is the IdP's own key for the identity, so it is tried first and
    // matched against the column logins join on.
    if let Some(external_id) = &payload.external_id {
        let found: Option<User> = users::table
            .filter(users::external_id.eq(external_id))
            .select(User::as_select())
            .first(conn)
            .optional()
            .map_err(ScimError::internal)?;
        if let Some(user) = found {
            return Ok(user);
        }
    }

    // Then userName, against user_name. Backfilled rows have
    // `user_name = external_id`, so an IdP that only ever sent the subject as
    // `userName` still matches here exactly as it used to.
    let found: Option<User> = users::table
        .filter(users::user_name.eq(&payload.user_name))
        .select(User::as_select())
        .first(conn)
        .optional()
        .map_err(ScimError::internal)?;
    if let Some(user) = found {
        return Ok(user);
    }

    // Email fallback: link to an existing (JIT-provisioned) row and RETAIN
    // its external_id — that is the OIDC sub logins join on.
    if let Some(email) = &payload.email {
        let found: Option<User> = users::table
            .filter(users::email.eq(email))
            .order(users::created_at.asc())
            .select(User::as_select())
            .first(conn)
            .optional()
            .map_err(ScimError::internal)?;
        if let Some(user) = found {
            return Ok(user);
        }
    }

    // Create: external_id = externalId // userName (the mapping note in
    // the module docs tells IdP admins to make this the OIDC sub).
    let email = payload.email.clone().ok_or_else(|| {
        ScimError::invalid_value(
            "cannot derive an email: provide emails[].value (or an email-shaped userName)",
        )
    })?;
    let external_id = payload
        .external_id
        .clone()
        .unwrap_or_else(|| payload.user_name.clone());
    let display_name = payload
        .display_name
        .clone()
        .unwrap_or_else(|| payload.user_name.clone());
    diesel::insert_into(users::table)
        .values(NewUser {
            external_id,
            user_name: payload.user_name.clone(),
            email,
            display_name,
        })
        .returning(User::as_returning())
        .get_result(conn)
        .map_err(|e| match e {
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            ) => ScimError::uniqueness("a user with this externalId/userName already exists"),
            e => ScimError::internal(e),
        })
}

/// Revoke the org membership (deprovision): LAST_ADMIN-guarded, activity-
/// logged, users row retained. Shared by PATCH `active:false`, PUT
/// `active:false`, and DELETE.
pub(crate) fn revoke_membership(
    conn: &mut PgConnection,
    ctx: &ScimContext,
    membership: &OrganizationMember,
    user: &User,
) -> Result<(), ScimError> {
    use kairos_db::schema::organization_members::dsl;
    if membership.role == OrgRole::Admin && admin_count(conn, ctx.org_id)? <= 1 {
        return Err(last_admin_error());
    }
    diesel::delete(
        dsl::organization_members
            .filter(dsl::organization_id.eq(ctx.org_id))
            .filter(dsl::user_id.eq(user.id)),
    )
    .execute(conn)
    .map_err(ScimError::internal)?;
    log_scim_activity(
        conn,
        ctx.actor_id,
        &ctx.token_name,
        ActivityAction::Delete,
        user.id,
        "membership",
        format!("member_remove:{}", user.email),
    )
}

/// Load the member (membership + user) or 404 — the resource set is the
/// org's memberships.
fn load_member(
    conn: &mut PgConnection,
    org_id: Uuid,
    user_id: Uuid,
) -> Result<(OrganizationMember, User), ScimError> {
    let membership = membership_of(conn, org_id, user_id)?;
    let user = load_user(conn, user_id)?;
    match (membership, user) {
        (Some(membership), Some(user)) => Ok((membership, user)),
        _ => Err(ScimError::not_found(format!(
            "no User resource with id {user_id} in this tenant"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /scim/v2/Users` — provision: link-or-create the `public.users`
/// row, create the membership. Already provisioned → 409 `uniqueness`.
pub(crate) async fn create_user(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    body: Bytes,
) -> Result<Response, ScimError> {
    let payload = parse_user_payload(&parse_json_body(&body)?)?;
    let resource = run_scim(&state, &ctx.slug.clone(), move |conn| {
        scim_transaction(conn, |conn| {
            use kairos_db::schema::organization_members;
            let user = resolve_or_create_user(conn, &payload)?;
            if membership_of(conn, ctx.org_id, user.id)?.is_some() {
                return Err(ScimError::uniqueness(format!(
                    "user {} is already provisioned in this tenant",
                    user.id
                )));
            }
            diesel::insert_into(organization_members::table)
                .values(NewOrganizationMember {
                    organization_id: ctx.org_id,
                    user_id: user.id,
                    role: OrgRole::Member,
                })
                .execute(conn)
                .map_err(ScimError::internal)?;
            log_scim_activity(
                conn,
                ctx.actor_id,
                &ctx.token_name,
                ActivityAction::Create,
                user.id,
                "membership",
                format!("member:{} role:member", user.email),
            )?;
            Ok(user_resource(&user, true))
        })
    })
    .await?;
    Ok(scim_response(StatusCode::CREATED, resource))
}

/// `GET /scim/v2/Users` — list the org's members; supports
/// `filter=userName eq "…"` / `externalId eq "…"` and startIndex/count.
pub(crate) async fn list_users(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    Query(params): Query<ListParams>,
) -> Result<Response, ScimError> {
    // KAIROS-T-0184 item 4: each attribute searches its OWN column. They used
    // to share `external_id`, so `userName eq "a login email"` against an
    // IdP with opaque subjects matched nothing — and an IdP that finds nothing
    // during reconciliation concludes the user is absent and re-creates them.
    enum UserFilter {
        UserName(String),
        ExternalId(String),
    }
    let user_filter = match &params.filter {
        None => None,
        Some(filter) => {
            let (attribute, value) = parse_eq_filter(filter)?;
            match attribute.as_str() {
                "username" => Some(UserFilter::UserName(value)),
                "externalid" => Some(UserFilter::ExternalId(value)),
                other => {
                    return Err(ScimError::invalid_filter(format!(
                        "unsupported filter attribute {other:?}: \
                         only userName and externalId are filterable"
                    )));
                }
            }
        }
    };
    let (start_index, count) = params.page();
    let response = run_scim(&state, &ctx.slug.clone(), move |conn| {
        use kairos_db::schema::{organization_members, users};
        let base = || {
            let mut query = organization_members::table
                .inner_join(users::table)
                .filter(organization_members::organization_id.eq(ctx.org_id))
                .into_boxed();
            match &user_filter {
                Some(UserFilter::UserName(value)) => {
                    query = query.filter(users::user_name.eq(value.clone()));
                }
                Some(UserFilter::ExternalId(value)) => {
                    query = query.filter(users::external_id.eq(value.clone()));
                }
                None => {}
            }
            query
        };
        let total: i64 = base()
            .count()
            .get_result(conn)
            .map_err(ScimError::internal)?;
        let rows: Vec<User> = base()
            .order(users::email.asc())
            .offset(start_index - 1)
            .limit(count)
            .select(User::as_select())
            .load(conn)
            .map_err(ScimError::internal)?;
        let resources = rows.iter().map(|u| user_resource(u, true)).collect();
        Ok(list_response(total, start_index, resources))
    })
    .await?;
    Ok(scim_response(StatusCode::OK, response))
}

/// `GET /scim/v2/Users/{id}`.
pub(crate) async fn get_user(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    Path(id): Path<String>,
) -> Result<Response, ScimError> {
    let user_id = parse_resource_id(&id)?;
    let resource = run_scim(&state, &ctx.slug.clone(), move |conn| {
        let (_membership, user) = load_member(conn, ctx.org_id, user_id)?;
        Ok(user_resource(&user, true))
    })
    .await?;
    Ok(scim_response(StatusCode::OK, resource))
}

/// One parsed User PATCH change.
enum UserChange {
    Active(bool),
    DisplayName(String),
}

/// Parse the RFC 7644 §3.5.2 PatchOp subset for Users (module docs).
fn parse_user_patch(body: &Value) -> Result<Vec<UserChange>, ScimError> {
    let schemas = body.get("schemas").and_then(Value::as_array);
    if !schemas.is_some_and(|s| s.iter().any(|v| v.as_str() == Some(PATCH_URN))) {
        return Err(ScimError::invalid_syntax(format!(
            "PATCH body must declare the {PATCH_URN} schema"
        )));
    }
    let operations = body
        .get("Operations")
        .and_then(Value::as_array)
        .ok_or_else(|| ScimError::invalid_syntax("PATCH body must carry an Operations array"))?;

    let mut changes = Vec::new();
    for operation in operations {
        let op = operation
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| ScimError::invalid_syntax("each operation needs an op"))?
            .to_ascii_lowercase();
        if op == "remove" {
            return Err(ScimError::invalid_path(
                "remove is not supported on User resources \
                 (deactivate with active: false instead)",
            ));
        }
        if op != "add" && op != "replace" {
            return Err(ScimError::invalid_value(format!("unsupported op {op:?}")));
        }
        let value = operation.get("value").unwrap_or(&Value::Null);
        match operation
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            // No path: the value object carries attributes. Genuinely unknown
            // attributes here are IGNORED (IdPs send many; module docs).
            //
            // Keys are matched CASE-INSENSITIVELY (KAIROS-T-0184 item 3). SCIM
            // attribute names are case-insensitive per RFC 7643 §2.1, and the
            // `path` form above already lowercased — but this arm matched
            // exactly, so `{"Active": false}` fell through to the ignore arm and
            // returned **200 with the user still provisioned**. A swallowed
            // deprovision is the worst possible shape for this bug: the IdP is
            // told the request succeeded, so it never retries, and the person
            // keeps their access.
            None => {
                let object = value.as_object().ok_or_else(|| {
                    ScimError::invalid_value("a pathless operation needs an object value")
                })?;
                for (key, attr_value) in object {
                    match key.to_ascii_lowercase().as_str() {
                        "active" => changes.push(UserChange::Active(
                            parse_scim_bool(attr_value).ok_or_else(|| {
                                ScimError::invalid_value("active must be a boolean")
                            })?,
                        )),
                        "displayname" => {
                            if let Some(name) = attr_value.as_str() {
                                changes.push(UserChange::DisplayName(name.to_string()));
                            }
                        }
                        _ => {} // ignored for IdP compatibility
                    }
                }
            }
            Some("active") => changes
                .push(UserChange::Active(parse_scim_bool(value).ok_or_else(
                    || ScimError::invalid_value("active must be a boolean"),
                )?)),
            Some("displayname") => changes.push(UserChange::DisplayName(
                value
                    .as_str()
                    .ok_or_else(|| ScimError::invalid_value("displayName must be a string"))?
                    .to_string(),
            )),
            Some(other) => {
                return Err(ScimError::invalid_path(format!(
                    "unsupported PATCH path {other:?}: only active and displayName are patchable"
                )));
            }
        }
    }
    Ok(changes)
}

/// `PATCH /scim/v2/Users/{id}` — `active: false` deprovisions immediately
/// (LAST_ADMIN-guarded); `displayName` updates the global profile.
pub(crate) async fn patch_user(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ScimError> {
    let user_id = parse_resource_id(&id)?;
    let changes = parse_user_patch(&parse_json_body(&body)?)?;
    let resource = run_scim(&state, &ctx.slug.clone(), move |conn| {
        scim_transaction(conn, |conn| {
            use kairos_db::schema::users;
            let (membership, user) = load_member(conn, ctx.org_id, user_id)?;
            let mut active = true;
            for change in &changes {
                match change {
                    UserChange::DisplayName(name) => {
                        diesel::update(users::table.filter(users::id.eq(user.id)))
                            .set((
                                users::display_name.eq(name),
                                users::updated_at.eq(diesel::dsl::now),
                            ))
                            .execute(conn)
                            .map_err(ScimError::internal)?;
                        log_scim_activity(
                            conn,
                            ctx.actor_id,
                            &ctx.token_name,
                            ActivityAction::Create,
                            user.id,
                            "user",
                            format!("user_profile:display_name user:{}", user.id),
                        )?;
                    }
                    UserChange::Active(false) if active => {
                        revoke_membership(conn, &ctx, &membership, &user)?;
                        active = false;
                    }
                    // active: true on an existing member is a no-op; the
                    // resource only exists while the membership does.
                    UserChange::Active(_) => {}
                }
            }
            let user = load_user(conn, user_id)?
                .ok_or_else(|| ScimError::internal("user row vanished mid-transaction"))?;
            Ok(user_resource(&user, active))
        })
    })
    .await?;
    Ok(scim_response(StatusCode::OK, resource))
}

/// `PUT /scim/v2/Users/{id}` — replace the writable profile subset.
///
/// `userName` IS mutable (KAIROS-T-0184). `externalId` is not: it is the OIDC
/// subject logins join on, and re-keying a live identity through a provisioning
/// call would lock the person out. A different `externalId` is 400 `mutability`,
/// and the message names the field and what to do instead — the old one said
/// "userName/externalId" without saying which, to an IdP admin who could see
/// neither.
///
/// `active: false` deprovisions.
pub(crate) async fn replace_user(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ScimError> {
    let user_id = parse_resource_id(&id)?;
    let payload = parse_user_payload(&parse_json_body(&body)?)?;
    let resource = run_scim(&state, &ctx.slug.clone(), move |conn| {
        scim_transaction(conn, |conn| {
            use kairos_db::schema::users;
            let (membership, user) = load_member(conn, ctx.org_id, user_id)?;
            if let Some(requested) = payload.external_id.as_ref()
                && requested != &user.external_id
            {
                return Err(ScimError::mutability(format!(
                    "externalId is immutable: it carries the OIDC subject that logins \
                     join on (KAIROS-A-0010), so changing it here would lock this user \
                     out rather than rename them. Stored {:?}, received {requested:?}. \
                     To change the login identifier, send `userName` — that is \
                     mutable. To re-key the identity itself, deprovision and \
                     re-provision.",
                    user.external_id
                )));
            }
            let display_name = payload
                .display_name
                .clone()
                .unwrap_or(user.display_name.clone());
            let email = payload.email.clone().unwrap_or(user.email.clone());
            let user_name = payload.user_name.clone();
            if display_name != user.display_name
                || email != user.email
                || user_name != user.user_name
            {
                diesel::update(users::table.filter(users::id.eq(user.id)))
                    .set((
                        users::user_name.eq(&user_name),
                        users::display_name.eq(&display_name),
                        users::email.eq(&email),
                        users::updated_at.eq(diesel::dsl::now),
                    ))
                    .execute(conn)
                    .map_err(|e| match e {
                        diesel::result::Error::DatabaseError(
                            diesel::result::DatabaseErrorKind::UniqueViolation,
                            _,
                        ) => ScimError::uniqueness(format!(
                            "userName {user_name:?} is already taken by another user"
                        )),
                        e => ScimError::internal(e),
                    })?;
                log_scim_activity(
                    conn,
                    ctx.actor_id,
                    &ctx.token_name,
                    ActivityAction::Create,
                    user.id,
                    "user",
                    format!("user_profile:replace user:{}", user.id),
                )?;
            }
            let mut active = true;
            if !payload.active {
                revoke_membership(conn, &ctx, &membership, &user)?;
                active = false;
            }
            let user = load_user(conn, user_id)?
                .ok_or_else(|| ScimError::internal("user row vanished mid-transaction"))?;
            Ok(user_resource(&user, active))
        })
    })
    .await?;
    Ok(scim_response(StatusCode::OK, resource))
}

/// `DELETE /scim/v2/Users/{id}` — revoke the membership (LAST_ADMIN-
/// guarded); the `public.users` row is retained for audit integrity.
pub(crate) async fn delete_user(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    Path(id): Path<String>,
) -> Result<Response, ScimError> {
    let user_id = parse_resource_id(&id)?;
    run_scim(&state, &ctx.slug.clone(), move |conn| {
        scim_transaction(conn, |conn| {
            let (membership, user) = load_member(conn, ctx.org_id, user_id)?;
            revoke_membership(conn, &ctx, &membership, &user)
        })
    })
    .await?;
    Ok(no_content())
}
