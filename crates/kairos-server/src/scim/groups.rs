//! `/scim/v2/Groups` (RFC 7644 §3): the group-mapping contract from the
//! [`super`] module docs —
//!
//! - `kairos-admins` (id = the organization UUID) ⇔
//!   `organization_members.role`: add promotes, remove demotes
//!   (LAST_ADMIN-guarded → 400 `mutability`);
//! - `kairos-team-<slug>` (id = the team UUID) ⇔ `team_members`; POST
//!   creates the team (+ delivery board, mirroring `/api/teams`), DELETE
//!   soft-deletes team + board.
//!
//! Group members must already be provisioned org members (IdPs push Users
//! before group memberships) — an unknown/unprovisioned `value` is 400
//! `invalidValue`.

use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use kairos_db::boards;
use kairos_db::models::enums::{ActivityAction, BoardLevel, OrgRole, TeamType};
use kairos_db::models::{NewTeam, NewTeamMember, Team, User};
use kairos_db::tenant::is_valid_slug;
use serde_json::{Value, json};
use uuid::Uuid;

use super::auth::ScimContext;
use super::error::{ScimError, scim_response};
use super::{
    GROUP_URN, ListParams, PATCH_URN, admin_count, last_admin_error, list_response, load_user,
    log_scim_activity, membership_of, no_content, parse_eq_filter, parse_json_body,
    parse_resource_id, run_scim, scim_transaction,
};
use crate::app::AppState;

/// The role-mapping group's displayName.
pub const ADMINS_GROUP_NAME: &str = "kairos-admins";
/// The team-group displayName prefix (`kairos-team-<slug>`).
pub const TEAM_GROUP_PREFIX: &str = "kairos-team-";

// ---------------------------------------------------------------------------
// Resolution + rendering
// ---------------------------------------------------------------------------

/// Which group a `/scim/v2/Groups/{id}` id names.
enum GroupTarget {
    /// `kairos-admins` — id is the organization UUID.
    Admins,
    /// `kairos-team-<slug>` — id is the team UUID (live teams only).
    Team(Team),
}

fn resolve_group(
    conn: &mut PgConnection,
    ctx: &ScimContext,
    id: Uuid,
) -> Result<GroupTarget, ScimError> {
    if id == ctx.org_id {
        return Ok(GroupTarget::Admins);
    }
    use kairos_db::schema::teams;
    let team: Option<Team> = teams::table
        .filter(teams::id.eq(id))
        .filter(teams::deleted_at.is_null())
        .select(Team::as_select())
        .first(conn)
        .optional()
        .map_err(ScimError::internal)?;
    team.map(GroupTarget::Team)
        .ok_or_else(|| ScimError::not_found(format!("no Group resource with id {id}")))
}

fn team_group_name(team: &Team) -> String {
    format!("{TEAM_GROUP_PREFIX}{}", team.slug)
}

/// The org's admins, ordered by email.
fn admins_members(conn: &mut PgConnection, org_id: Uuid) -> Result<Vec<User>, ScimError> {
    use kairos_db::schema::{organization_members, users};
    organization_members::table
        .inner_join(users::table)
        .filter(organization_members::organization_id.eq(org_id))
        .filter(organization_members::role.eq(OrgRole::Admin))
        .order(users::email.asc())
        .select(User::as_select())
        .load(conn)
        .map_err(ScimError::internal)
}

/// A team's members, ordered by email (two queries: `team_members` and
/// `public.users` live in different allow-groups, so no cross join).
fn team_member_users(conn: &mut PgConnection, team_id: Uuid) -> Result<Vec<User>, ScimError> {
    use kairos_db::schema::{team_members, users};
    let user_ids: Vec<Uuid> = team_members::table
        .filter(team_members::team_id.eq(team_id))
        .select(team_members::user_id)
        .load(conn)
        .map_err(ScimError::internal)?;
    users::table
        .filter(users::id.eq_any(user_ids))
        .order(users::email.asc())
        .select(User::as_select())
        .load(conn)
        .map_err(ScimError::internal)
}

fn group_resource(id: Uuid, display_name: &str, members: &[User]) -> Value {
    json!({
        "schemas": [GROUP_URN],
        "id": id.to_string(),
        "displayName": display_name,
        "members": members.iter().map(|u| json!({
            "value": u.id.to_string(),
            "display": u.display_name,
            "$ref": format!("/scim/v2/Users/{}", u.id),
        })).collect::<Vec<_>>(),
        "meta": {
            "resourceType": "Group",
            "location": format!("/scim/v2/Groups/{id}"),
        },
    })
}

fn render_target(
    conn: &mut PgConnection,
    ctx: &ScimContext,
    target: &GroupTarget,
) -> Result<Value, ScimError> {
    match target {
        GroupTarget::Admins => Ok(group_resource(
            ctx.org_id,
            ADMINS_GROUP_NAME,
            &admins_members(conn, ctx.org_id)?,
        )),
        GroupTarget::Team(team) => Ok(group_resource(
            team.id,
            &team_group_name(team),
            &team_member_users(conn, team.id)?,
        )),
    }
}

// ---------------------------------------------------------------------------
// Payload parsing
// ---------------------------------------------------------------------------

/// Member entries are `{"value": "<user uuid>"}` objects (or bare strings).
fn parse_member_list(value: &Value) -> Result<Vec<Uuid>, ScimError> {
    let entries = value
        .as_array()
        .ok_or_else(|| ScimError::invalid_value("members must be an array"))?;
    entries
        .iter()
        .map(|entry| {
            let id = match entry {
                Value::String(s) => s.as_str(),
                other => other
                    .get("value")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ScimError::invalid_value("each member needs a value"))?,
            };
            Uuid::parse_str(id).map_err(|_| {
                ScimError::invalid_value(format!("member value {id:?} is not a User id"))
            })
        })
        .collect()
}

/// One parsed Group PATCH change.
enum GroupChange {
    Add(Vec<Uuid>),
    Remove(Vec<Uuid>),
    RemoveAll,
    Replace(Vec<Uuid>),
    /// `displayName` write — applied as "must equal the current name"
    /// (renames are 400 `mutability`).
    DisplayName(String),
}

/// `members[value eq "<uuid>"]` → the uuid.
fn parse_member_value_path(path: &str) -> Option<&str> {
    path.strip_prefix("members[value eq \"")?
        .strip_suffix("\"]")
}

/// Parse the RFC 7644 §3.5.2 PatchOp subset for Groups (module docs).
fn parse_group_patch(body: &Value) -> Result<Vec<GroupChange>, ScimError> {
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
        let value = operation.get("value").unwrap_or(&Value::Null);
        let path = operation.get("path").and_then(Value::as_str);

        match (op.as_str(), path) {
            ("add" | "replace", None) => {
                let object = value.as_object().ok_or_else(|| {
                    ScimError::invalid_value("a pathless operation needs an object value")
                })?;
                if let Some(name) = object.get("displayName").and_then(Value::as_str) {
                    changes.push(GroupChange::DisplayName(name.to_string()));
                }
                if let Some(members) = object.get("members") {
                    let list = parse_member_list(members)?;
                    changes.push(if op == "add" {
                        GroupChange::Add(list)
                    } else {
                        GroupChange::Replace(list)
                    });
                }
            }
            ("add", Some(path)) if path.eq_ignore_ascii_case("members") => {
                changes.push(GroupChange::Add(parse_member_list(value)?));
            }
            ("replace", Some(path)) if path.eq_ignore_ascii_case("members") => {
                changes.push(GroupChange::Replace(parse_member_list(value)?));
            }
            ("replace", Some(path)) if path.eq_ignore_ascii_case("displayname") => {
                let name = value
                    .as_str()
                    .ok_or_else(|| ScimError::invalid_value("displayName must be a string"))?;
                changes.push(GroupChange::DisplayName(name.to_string()));
            }
            ("remove", Some(path)) if path.eq_ignore_ascii_case("members") => {
                changes.push(match value {
                    Value::Null => GroupChange::RemoveAll,
                    list => GroupChange::Remove(parse_member_list(list)?),
                });
            }
            ("remove", Some(path)) => match parse_member_value_path(path) {
                Some(id) => {
                    let id = Uuid::parse_str(id).map_err(|_| {
                        ScimError::invalid_value(format!("member value {id:?} is not a User id"))
                    })?;
                    changes.push(GroupChange::Remove(vec![id]));
                }
                None => {
                    return Err(ScimError::invalid_path(format!(
                        "unsupported PATCH path {path:?}: only members (and \
                         members[value eq \"…\"]) are patchable"
                    )));
                }
            },
            ("add" | "replace", Some(path)) => {
                return Err(ScimError::invalid_path(format!(
                    "unsupported PATCH path {path:?}: only members and displayName are patchable"
                )));
            }
            ("remove", None) => {
                return Err(ScimError::invalid_path("remove needs a path"));
            }
            (other, _) => {
                return Err(ScimError::invalid_value(format!(
                    "unsupported op {other:?}"
                )));
            }
        }
    }
    Ok(changes)
}

// ---------------------------------------------------------------------------
// Membership mechanics
// ---------------------------------------------------------------------------

/// A group member must already be a provisioned org member.
fn require_provisioned(
    conn: &mut PgConnection,
    ctx: &ScimContext,
    user_id: Uuid,
) -> Result<User, ScimError> {
    let membership = membership_of(conn, ctx.org_id, user_id)?;
    let user = load_user(conn, user_id)?;
    match (membership, user) {
        (Some(_), Some(user)) => Ok(user),
        _ => Err(ScimError::invalid_value(format!(
            "user {user_id} is not provisioned in this tenant; \
             push the User resource before its group memberships"
        ))),
    }
}

fn set_org_role(
    conn: &mut PgConnection,
    ctx: &ScimContext,
    user: &User,
    role: OrgRole,
) -> Result<(), ScimError> {
    use kairos_db::schema::organization_members::dsl;
    diesel::update(
        dsl::organization_members
            .filter(dsl::organization_id.eq(ctx.org_id))
            .filter(dsl::user_id.eq(user.id)),
    )
    .set(dsl::role.eq(role))
    .execute(conn)
    .map_err(ScimError::internal)?;
    log_scim_activity(
        conn,
        ctx.actor_id,
        &ctx.token_name,
        ActivityAction::Create,
        user.id,
        "membership",
        format!("membership_role:->{role} user:{}", user.id),
    )
}

/// Apply one admins-group member change set.
fn apply_admins_change(
    conn: &mut PgConnection,
    ctx: &ScimContext,
    change: &GroupChange,
) -> Result<(), ScimError> {
    let current_admin_ids = |conn: &mut PgConnection| -> Result<Vec<Uuid>, ScimError> {
        Ok(admins_members(conn, ctx.org_id)?
            .iter()
            .map(|u| u.id)
            .collect())
    };
    let promote = |conn: &mut PgConnection, user_id: Uuid| -> Result<(), ScimError> {
        let user = require_provisioned(conn, ctx, user_id)?;
        let membership = membership_of(conn, ctx.org_id, user_id)?
            .ok_or_else(|| ScimError::internal("membership vanished mid-transaction"))?;
        if membership.role != OrgRole::Admin {
            set_org_role(conn, ctx, &user, OrgRole::Admin)?;
        }
        Ok(())
    };
    let demote = |conn: &mut PgConnection, user_id: Uuid| -> Result<(), ScimError> {
        let Some(membership) = membership_of(conn, ctx.org_id, user_id)? else {
            return Ok(()); // not a member: nothing to demote
        };
        if membership.role != OrgRole::Admin {
            return Ok(());
        }
        if admin_count(conn, ctx.org_id)? <= 1 {
            return Err(last_admin_error());
        }
        let user = load_user(conn, user_id)?
            .ok_or_else(|| ScimError::internal("user vanished mid-transaction"))?;
        set_org_role(conn, ctx, &user, OrgRole::Member)
    };

    match change {
        GroupChange::Add(list) => list.iter().try_for_each(|&id| promote(conn, id)),
        GroupChange::Remove(list) => list.iter().try_for_each(|&id| demote(conn, id)),
        GroupChange::RemoveAll => current_admin_ids(conn)?
            .iter()
            .try_for_each(|&id| demote(conn, id)),
        GroupChange::Replace(list) => {
            // Promote first, then demote: swapping the admin set must not
            // trip the LAST_ADMIN guard mid-way.
            let current = current_admin_ids(conn)?;
            list.iter()
                .filter(|id| !current.contains(id))
                .try_for_each(|&id| promote(conn, id))?;
            current
                .iter()
                .filter(|id| !list.contains(id))
                .try_for_each(|&id| demote(conn, id))
        }
        GroupChange::DisplayName(_) => unreachable!("handled by the caller"),
    }
}

/// Apply one team-group member change set.
fn apply_team_change(
    conn: &mut PgConnection,
    ctx: &ScimContext,
    team: &Team,
    change: &GroupChange,
) -> Result<(), ScimError> {
    use kairos_db::schema::team_members::dsl;
    let add = |conn: &mut PgConnection, user_id: Uuid| -> Result<(), ScimError> {
        let user = require_provisioned(conn, ctx, user_id)?;
        let inserted = diesel::insert_into(dsl::team_members)
            .values(NewTeamMember {
                team_id: team.id,
                user_id,
            })
            .on_conflict_do_nothing()
            .execute(conn)
            .map_err(ScimError::internal)?;
        if inserted > 0 {
            log_scim_activity(
                conn,
                ctx.actor_id,
                &ctx.token_name,
                ActivityAction::Create,
                user.id,
                "team_membership",
                format!("team:{} member:{}", team.slug, user.email),
            )?;
        }
        Ok(())
    };
    let remove = |conn: &mut PgConnection, user_id: Uuid| -> Result<(), ScimError> {
        let removed = diesel::delete(
            dsl::team_members
                .filter(dsl::team_id.eq(team.id))
                .filter(dsl::user_id.eq(user_id)),
        )
        .execute(conn)
        .map_err(ScimError::internal)?;
        if removed > 0 {
            log_scim_activity(
                conn,
                ctx.actor_id,
                &ctx.token_name,
                ActivityAction::Delete,
                user_id,
                "team_membership",
                format!("team:{} member_remove:{user_id}", team.slug),
            )?;
        }
        Ok(())
    };

    match change {
        GroupChange::Add(list) => list.iter().try_for_each(|&id| add(conn, id)),
        GroupChange::Remove(list) => list.iter().try_for_each(|&id| remove(conn, id)),
        GroupChange::RemoveAll => {
            let current: Vec<Uuid> = team_member_users(conn, team.id)?
                .iter()
                .map(|u| u.id)
                .collect();
            current.iter().try_for_each(|&id| remove(conn, id))
        }
        GroupChange::Replace(list) => {
            let current: Vec<Uuid> = team_member_users(conn, team.id)?
                .iter()
                .map(|u| u.id)
                .collect();
            list.iter()
                .filter(|id| !current.contains(id))
                .try_for_each(|&id| add(conn, id))?;
            current
                .iter()
                .filter(|id| !list.contains(id))
                .try_for_each(|&id| remove(conn, id))
        }
        GroupChange::DisplayName(_) => unreachable!("handled by the caller"),
    }
}

/// Apply a parsed change list to a resolved group. `displayName` writes
/// must equal the current name (renames → 400 `mutability`).
fn apply_group_changes(
    conn: &mut PgConnection,
    ctx: &ScimContext,
    target: &GroupTarget,
    changes: &[GroupChange],
) -> Result<(), ScimError> {
    let current_name = match target {
        GroupTarget::Admins => ADMINS_GROUP_NAME.to_string(),
        GroupTarget::Team(team) => team_group_name(team),
    };
    for change in changes {
        match change {
            GroupChange::DisplayName(name) if name != &current_name => {
                return Err(ScimError::mutability(format!(
                    "group displayName is immutable ({current_name:?}); \
                     rename teams via /api/teams"
                )));
            }
            GroupChange::DisplayName(_) => {}
            member_change => match target {
                GroupTarget::Admins => apply_admins_change(conn, ctx, member_change)?,
                GroupTarget::Team(team) => apply_team_change(conn, ctx, team, member_change)?,
            },
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /scim/v2/Groups` — `kairos-admins` + one group per live team;
/// supports `filter=displayName eq "…"` and startIndex/count.
pub(crate) async fn list_groups(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    Query(params): Query<ListParams>,
) -> Result<Response, ScimError> {
    let name_filter = match &params.filter {
        None => None,
        Some(filter) => {
            let (attribute, value) = parse_eq_filter(filter)?;
            if attribute != "displayname" {
                return Err(ScimError::invalid_filter(format!(
                    "unsupported filter attribute {attribute:?}: \
                     only displayName is filterable on Groups"
                )));
            }
            Some(value)
        }
    };
    let (start_index, count) = params.page();
    let response = run_scim(&state, &ctx.slug.clone(), move |conn| {
        use kairos_db::schema::teams;
        let mut resources: Vec<Value> = Vec::new();
        if name_filter
            .as_deref()
            .is_none_or(|f| f == ADMINS_GROUP_NAME)
        {
            resources.push(group_resource(
                ctx.org_id,
                ADMINS_GROUP_NAME,
                &admins_members(conn, ctx.org_id)?,
            ));
        }
        let live_teams: Vec<Team> = teams::table
            .filter(teams::deleted_at.is_null())
            .order(teams::slug.asc())
            .select(Team::as_select())
            .load(conn)
            .map_err(ScimError::internal)?;
        for team in &live_teams {
            let name = team_group_name(team);
            if name_filter.as_deref().is_none_or(|f| f == name) {
                resources.push(group_resource(
                    team.id,
                    &name,
                    &team_member_users(conn, team.id)?,
                ));
            }
        }
        let total = resources.len() as i64;
        let page: Vec<Value> = resources
            .into_iter()
            .skip((start_index - 1) as usize)
            .take(count as usize)
            .collect();
        Ok(list_response(total, start_index, page))
    })
    .await?;
    Ok(scim_response(StatusCode::OK, response))
}

/// `GET /scim/v2/Groups/{id}`.
pub(crate) async fn get_group(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    Path(id): Path<String>,
) -> Result<Response, ScimError> {
    let group_id = parse_resource_id(&id)?;
    let resource = run_scim(&state, &ctx.slug.clone(), move |conn| {
        let target = resolve_group(conn, &ctx, group_id)?;
        render_target(conn, &ctx, &target)
    })
    .await?;
    Ok(scim_response(StatusCode::OK, resource))
}

/// `POST /scim/v2/Groups` — create a team-mapped group
/// (`kairos-team-<slug>`, plus its delivery board) with optional initial
/// members. `kairos-admins` always exists → 409 `uniqueness`; any other
/// naming → 400 `invalidValue`.
pub(crate) async fn create_group(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    body: Bytes,
) -> Result<Response, ScimError> {
    let body = parse_json_body(&body)?;
    let display_name = body
        .get("displayName")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ScimError::invalid_value("displayName is required"))?
        .to_string();
    let members = match body.get("members") {
        None | Some(Value::Null) => Vec::new(),
        Some(value) => parse_member_list(value)?,
    };
    if display_name == ADMINS_GROUP_NAME {
        return Err(ScimError::uniqueness(format!(
            "{ADMINS_GROUP_NAME:?} is the built-in role-mapping group and always exists"
        )));
    }
    let team_slug = display_name
        .strip_prefix(TEAM_GROUP_PREFIX)
        .filter(|slug| is_valid_slug(slug))
        .ok_or_else(|| {
            ScimError::invalid_value(format!(
                "unsupported group displayName {display_name:?}: Kairos maps \
                 {ADMINS_GROUP_NAME:?} to the org-admin role and \
                 \"{TEAM_GROUP_PREFIX}<slug>\" to teams (slug: ^[a-z][a-z0-9_-]{{1,62}}$)"
            ))
        })?
        .to_string();

    let resource = run_scim(&state, &ctx.slug.clone(), move |conn| {
        scim_transaction(conn, |conn| {
            use kairos_db::schema::teams;
            let team: Team = diesel::insert_into(teams::table)
                .values(NewTeam {
                    name: team_slug.clone(),
                    slug: team_slug.clone(),
                    team_type: TeamType::StreamAligned,
                })
                .returning(Team::as_returning())
                .get_result(conn)
                .map_err(|e| match e {
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::UniqueViolation,
                        _,
                    ) => ScimError::uniqueness(format!(
                        "a team with slug {team_slug:?} already exists"
                    )),
                    e => ScimError::internal(e),
                })?;
            // The team's delivery board is created WITH the team, from the
            // seeded defaults — the same rule as POST /api/teams.
            boards::create_board(
                conn,
                BoardLevel::Delivery,
                &format!("{} Delivery", team.name),
                &format!("{}-delivery", team.slug),
                Some(team.id),
                Some(ctx.actor_id),
            )
            .map_err(|e| match e {
                boards::BoardError::Database(ref db)
                    if matches!(
                        db,
                        diesel::result::Error::DatabaseError(
                            diesel::result::DatabaseErrorKind::UniqueViolation,
                            _
                        )
                    ) =>
                {
                    ScimError::uniqueness(format!(
                        "a board with slug \"{}-delivery\" already exists",
                        team.slug
                    ))
                }
                e => ScimError::internal(e),
            })?;
            log_scim_activity(
                conn,
                ctx.actor_id,
                &ctx.token_name,
                ActivityAction::Create,
                team.id,
                "team",
                format!("team:{} delivery_board:{}-delivery", team.slug, team.slug),
            )?;
            apply_team_change(conn, &ctx, &team, &GroupChange::Add(members.clone()))?;
            let member_users = team_member_users(conn, team.id)?;
            Ok(group_resource(
                team.id,
                &team_group_name(&team),
                &member_users,
            ))
        })
    })
    .await?;
    Ok(scim_response(StatusCode::CREATED, resource))
}

/// `PATCH /scim/v2/Groups/{id}` — member add/remove/replace (module docs).
pub(crate) async fn patch_group(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ScimError> {
    let group_id = parse_resource_id(&id)?;
    let changes = parse_group_patch(&parse_json_body(&body)?)?;
    let resource = run_scim(&state, &ctx.slug.clone(), move |conn| {
        scim_transaction(conn, |conn| {
            let target = resolve_group(conn, &ctx, group_id)?;
            apply_group_changes(conn, &ctx, &target, &changes)?;
            render_target(conn, &ctx, &target)
        })
    })
    .await?;
    Ok(scim_response(StatusCode::OK, resource))
}

/// `PUT /scim/v2/Groups/{id}` — replace the member set (`displayName` must
/// match the current name; renames → 400 `mutability`).
pub(crate) async fn replace_group(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ScimError> {
    let group_id = parse_resource_id(&id)?;
    let body = parse_json_body(&body)?;
    let mut changes = Vec::new();
    if let Some(name) = body.get("displayName").and_then(Value::as_str) {
        changes.push(GroupChange::DisplayName(name.to_string()));
    }
    changes.push(GroupChange::Replace(match body.get("members") {
        None | Some(Value::Null) => Vec::new(),
        Some(value) => parse_member_list(value)?,
    }));
    let resource = run_scim(&state, &ctx.slug.clone(), move |conn| {
        scim_transaction(conn, |conn| {
            let target = resolve_group(conn, &ctx, group_id)?;
            apply_group_changes(conn, &ctx, &target, &changes)?;
            render_target(conn, &ctx, &target)
        })
    })
    .await?;
    Ok(scim_response(StatusCode::OK, resource))
}

/// How many LIVE workflow items sit on `board_id` (the board-empty rule,
/// retyped for the SCIM surface — mirrors `api::org::count_live_board_items`,
/// KAIROS-I-0012: soft-deleted cards no longer block).
fn count_board_items(conn: &mut PgConnection, board_id: Uuid) -> Result<i64, ScimError> {
    use kairos_db::schema::{adrs, initiatives, strategies, tasks};
    let mut total: i64 = 0;
    total += strategies::table
        .filter(strategies::board_id.eq(board_id))
        .filter(strategies::deleted_at.is_null())
        .count()
        .get_result::<i64>(conn)
        .map_err(ScimError::internal)?;
    total += initiatives::table
        .filter(initiatives::board_id.eq(board_id))
        .filter(initiatives::deleted_at.is_null())
        .count()
        .get_result::<i64>(conn)
        .map_err(ScimError::internal)?;
    total += tasks::table
        .filter(tasks::board_id.eq(board_id))
        .filter(tasks::deleted_at.is_null())
        .count()
        .get_result::<i64>(conn)
        .map_err(ScimError::internal)?;
    total += adrs::table
        .filter(adrs::board_id.eq(board_id))
        .filter(adrs::deleted_at.is_null())
        .count()
        .get_result::<i64>(conn)
        .map_err(ScimError::internal)?;
    Ok(total)
}

/// `DELETE /scim/v2/Groups/{id}` — soft-delete the team + its delivery
/// board (refused while the board holds items). The built-in
/// `kairos-admins` group cannot be deleted.
pub(crate) async fn delete_group(
    State(state): State<AppState>,
    Extension(ctx): Extension<ScimContext>,
    Path(id): Path<String>,
) -> Result<Response, ScimError> {
    let group_id = parse_resource_id(&id)?;
    run_scim(&state, &ctx.slug.clone(), move |conn| {
        use kairos_db::schema::{boards as boards_schema, teams};
        let team = match resolve_group(conn, &ctx, group_id)? {
            GroupTarget::Admins => {
                return Err(ScimError::mutability(format!(
                    "{ADMINS_GROUP_NAME:?} is the built-in role-mapping group \
                     and cannot be deleted"
                )));
            }
            GroupTarget::Team(team) => team,
        };
        let board_id: Option<Uuid> = boards_schema::table
            .filter(boards_schema::team_id.eq(team.id))
            .filter(boards_schema::board_level.eq(BoardLevel::Delivery))
            .filter(boards_schema::deleted_at.is_null())
            .select(boards_schema::id)
            .first(conn)
            .optional()
            .map_err(ScimError::internal)?;
        if let Some(board_id) = board_id {
            let item_count = count_board_items(conn, board_id)?;
            if item_count > 0 {
                return Err(ScimError::mutability(format!(
                    "team {:?}'s delivery board still contains {item_count} item(s); \
                     move or delete them before removing the group",
                    team.slug
                )));
            }
        }
        scim_transaction(conn, |conn| {
            diesel::update(teams::table.filter(teams::id.eq(team.id)))
                .set((
                    teams::deleted_at.eq(diesel::dsl::now),
                    teams::updated_at.eq(diesel::dsl::now),
                ))
                .execute(conn)
                .map_err(ScimError::internal)?;
            if let Some(board_id) = board_id {
                diesel::update(boards_schema::table.filter(boards_schema::id.eq(board_id)))
                    .set((
                        boards_schema::deleted_at.eq(diesel::dsl::now),
                        boards_schema::updated_at.eq(diesel::dsl::now),
                    ))
                    .execute(conn)
                    .map_err(ScimError::internal)?;
            }
            log_scim_activity(
                conn,
                ctx.actor_id,
                &ctx.token_name,
                ActivityAction::Delete,
                team.id,
                "team",
                format!("team:{}", team.slug),
            )
        })
    })
    .await?;
    Ok(no_content())
}
