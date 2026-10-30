//! The work of a person (KAIROS-T-0489, COLLIERY-I-0624): `GET
//! /api/me/work`. The MCP tool `my_work`, the CLI `kairos me` and the "for
//! me" page read the same answer ([`work_for`]). The rules of the lanes and
//! of the states are in `kairos_db::my_work`.
//!
//! The caller is the person: a request with an agent key gives the work of
//! the person of the key. A service account owns nothing and has no claim,
//! so its work is empty.
//!
//! # The stand-up of a team (KAIROS-T-0722, COLLIERY-I-0663, KAIROS-A-0077)
//!
//! Three read-only routes give a stand-up: the people of the team, the
//! work of one person in the team, and the tasks of the team that no one
//! claims. The rules are in `kairos_db::my_work`. Each member of the
//! organization can read the work of each person (Dylan, 2026-10-28): the
//! page is a prop for the conversation, and it writes nothing.

use axum::extract::Path;
use axum::extract::{Extension, State};
use axum::routing::get;
use axum::{Json, Router};
use kairos_client::types_work::{
    MyWork, Standup, StandupPerson, StandupTeam, UnclaimedWork, WorkBlocker, WorkBoard, WorkItem,
};
use kairos_db::my_work;
use uuid::Uuid;

use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::auth::AuthContext;
use crate::middleware::tenant::TenantContext;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/me/work", get(get_my_work))
        .route("/api/teams/{id}/standup", get(get_standup))
        .route(
            "/api/teams/{id}/standup/people/{user_id}",
            get(get_standup_person),
        )
        .route(
            "/api/teams/{id}/standup/unclaimed",
            get(get_standup_unclaimed),
        )
}

/// The work of `user`, as the wire type. Five queries and the names of the
/// people.
pub(crate) fn work_for(
    conn: &mut diesel::pg::PgConnection,
    user: Uuid,
) -> Result<MyWork, ApiError> {
    let work = my_work::my_work(conn, user).map_err(ApiError::internal)?;
    to_wire(conn, work)
}

/// The two lanes as the wire type, with the names of the people. One query.
fn to_wire(conn: &mut diesel::pg::PgConnection, work: my_work::MyWork) -> Result<MyWork, ApiError> {
    let names = names_of(conn, work.initiatives.iter().chain(work.tasks.iter()))?;
    Ok(MyWork {
        initiatives: work
            .initiatives
            .into_iter()
            .map(|item| item_to_wire(item, &names))
            .collect(),
        tasks: work
            .tasks
            .into_iter()
            .map(|item| item_to_wire(item, &names))
            .collect(),
    })
}

/// The display names of the people of the claims and the owners of
/// `items`. One query.
fn names_of<'a>(
    conn: &mut diesel::pg::PgConnection,
    items: impl Iterator<Item = &'a my_work::WorkItem>,
) -> Result<std::collections::HashMap<Uuid, String>, ApiError> {
    let people: Vec<Uuid> = items
        .flat_map(|item| {
            item.claim
                .iter()
                .map(|c| c.user_id)
                .chain(item.owner.iter().map(|o| o.user_id))
        })
        .collect();
    kairos_db::task_claims::display_names(conn, &people).map_err(ApiError::internal)
}

/// One item of the work as the wire type, with the names of [`names_of`].
fn item_to_wire(
    item: my_work::WorkItem,
    names: &std::collections::HashMap<Uuid, String>,
) -> WorkItem {
    WorkItem {
        id: item.id.to_string(),
        short_code: item.short_code,
        title: item.title,
        entity_type: item.entity_type,
        state: item.state.as_str().to_string(),
        reasons: item
            .reasons
            .iter()
            .map(|r| r.as_str().to_string())
            .collect(),
        board: WorkBoard {
            id: item.board_id.to_string(),
            slug: item.board_slug,
            name: item.board_name,
        },
        column_id: item.column_id.to_string(),
        column_name: item.column_name,
        claim: item
            .claim
            .as_ref()
            .map(|c| crate::claims::claim_dto(c, names)),
        owner: item
            .owner
            .as_ref()
            .map(|o| crate::owners::owner_dto(o, names)),
        blockers: item
            .blockers
            .into_iter()
            .map(|b| WorkBlocker {
                short_code: b.short_code,
                title: b.title,
            })
            .collect(),
    }
}

/// The live team that `reference` names (a slug or an id), with its id, or
/// 404. Two queries.
fn standup_team(
    conn: &mut diesel::pg::PgConnection,
    reference: &str,
) -> Result<(Uuid, StandupTeam), ApiError> {
    use diesel::prelude::*;
    use kairos_db::schema::teams;
    let id = crate::api::org::teams::team_by_ref(conn, reference)?;
    let (slug, name): (String, String) = teams::table
        .filter(teams::id.eq(id))
        .select((teams::slug, teams::name))
        .first(conn)
        .map_err(ApiError::internal)?;
    Ok((
        id,
        StandupTeam {
            id: id.to_string(),
            slug,
            name,
        },
    ))
}

/// The person of the id `reference`: a person of the organization `slug`,
/// or 404. A service account is not a person.
fn standup_person(
    conn: &mut diesel::pg::PgConnection,
    slug: &str,
    reference: &str,
) -> Result<Uuid, ApiError> {
    let not_found = || {
        ApiError::not_found(format!(
            "No person of the organization has the id {reference:?}. Send the user_id of a \
             person of GET /api/teams/{{id}}/standup."
        ))
        .with_details(serde_json::json!({ "field": "user_id" }))
    };
    let user = Uuid::parse_str(reference).map_err(|_| not_found())?;
    let member = kairos_db::abac::is_org_member(conn, slug, user).map_err(ApiError::internal)?;
    let person = kairos_db::task_claims::is_person(conn, user).map_err(ApiError::internal)?;
    if member && person {
        Ok(user)
    } else {
        Err(not_found())
    }
}

/// The team of a stand-up and its people.
///
/// The people are the members of the team, by display name. Some people
/// are not members but claim a task of the team or own an initiative of
/// the team. They get a turn too, with `member: false`. A service account
/// is not in the list. The page shuffles the list.
#[utoipa::path(
    get,
    path = "/api/teams/{id}/standup",
    tag = "teams",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
    responses(
        (status = 200, description = "The team and its people", body = Standup),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_standup(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Standup>, ApiError> {
    let standup = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (team_id, team) = standup_team(conn, &id)?;
            let people = my_work::team_people(conn, team_id)
                .map_err(ApiError::internal)?
                .into_iter()
                .map(|p| StandupPerson {
                    user_id: p.user_id.to_string(),
                    user_name: p.user_name,
                    display_name: p.display_name,
                    member: p.member,
                })
                .collect();
            Ok(Standup { team, people })
        })
        .await?;
    Ok(Json(standup))
}

/// The work of one person in a team, in two lanes.
///
/// - Initiatives: the initiatives that the person owns and that have the
///   team (from their tasks, or set by hand).
/// - Tasks: the tasks on the boards of the team that the person, or an
///   agent key of the person, claims.
///
/// Any member of the organization can read the work of any person. The
/// person does not have to be a member of the team.
#[utoipa::path(
    get,
    path = "/api/teams/{id}/standup/people/{user_id}",
    tag = "teams",
    params(
        ("id" = String, Path, description = "The slug or the id (UUID) of the team"),
        ("user_id" = String, Path, description = "The id (UUID) of the person"),
    ),
    responses(
        (status = 200, description = "The two lanes of the work of the person in the team", body = MyWork),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team or person", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_standup_person(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<Json<MyWork>, ApiError> {
    let slug = tenant.slug.clone();
    let work = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (team_id, _) = standup_team(conn, &id)?;
            let user = standup_person(conn, &slug, &user_id)?;
            let work = my_work::team_work(conn, team_id, user).map_err(ApiError::internal)?;
            to_wire(conn, work)
        })
        .await?;
    Ok(Json(work))
}

/// The last stop of a stand-up. It gives the tasks on the boards of the
/// team that no one claims, in the state Todo or Blocked. A task in Backlog
/// is not in it.
#[utoipa::path(
    get,
    path = "/api/teams/{id}/standup/unclaimed",
    tag = "teams",
    params(("id" = String, Path, description = "The slug or the id (UUID) of the team")),
    responses(
        (status = 200, description = "The unclaimed tasks of the team", body = UnclaimedWork),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 404, description = "Unknown team", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_standup_unclaimed(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<UnclaimedWork>, ApiError> {
    let work = state
        .blocking
        .run(&tenant.slug, move |conn| {
            let (team_id, _) = standup_team(conn, &id)?;
            let items = my_work::unclaimed_work(conn, team_id).map_err(ApiError::internal)?;
            let names = names_of(conn, items.iter())?;
            let tasks = items
                .into_iter()
                .map(|item| item_to_wire(item, &names))
                .collect();
            Ok(UnclaimedWork { tasks })
        })
        .await?;
    Ok(Json(work))
}

/// The work of the caller, in two lanes.
///
/// - Initiatives: the strategies and the initiatives that the caller owns.
/// - Tasks: the claims of the caller and of the agent keys of the caller.
///   The Todo, Active and Blocked tasks under an initiative of the caller.
///   The tasks that one of these tasks blocks.
///
/// Each item has a state (`todo`, `active`, `blocked`) and its open
/// blockers. The answer has no item in a done column.
#[utoipa::path(
    get,
    path = "/api/me/work",
    tag = "me",
    responses(
        (status = 200, description = "The two lanes of the work of the caller", body = MyWork),
        (status = 401, description = "Missing/invalid token", body = kairos_client::types::ErrorEnvelope),
        (status = 403, description = "Not a member of the organization", body = kairos_client::types::ErrorEnvelope),
    ),
)]
pub(crate) async fn get_my_work(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<MyWork>, ApiError> {
    let user = auth.user_id;
    let work = state
        .blocking
        .run(&tenant.slug, move |conn| work_for(conn, user))
        .await?;
    Ok(Json(work))
}

/// The states in the order of the page: Todo, Active, Blocked.
const STATES: [(&str, &str); 3] = [
    ("todo", "Todo"),
    ("active", "Active"),
    ("blocked", "Blocked"),
];

/// One item of the work as a line of text (the MCP tool `my_work`).
fn item_line(item: &WorkItem) -> String {
    let mut line = format!(
        "- {} \u{2014} {} ({} / {}",
        item.short_code, item.title, item.board.slug, item.column_name
    );
    if let Some(claim) = &item.claim {
        line.push_str(&format!("; claimed by {}", claim.display_name));
        if claim.agent {
            line.push_str(" (agent)");
        }
    } else if item.entity_type == "task" {
        line.push_str("; no claim");
    }
    if let Some(owner) = &item.owner {
        line.push_str(&format!("; owner: {}", owner.display_name));
    }
    line.push_str(&format!(") [{}]\n", item.reasons.join(", ")));
    for blocker in &item.blockers {
        line.push_str(&format!(
            "  blocked by {} \u{2014} {}\n",
            blocker.short_code, blocker.title
        ));
    }
    line
}

/// The work as text: each lane, then each state, then each item.
pub(crate) fn render_text(work: &MyWork) -> String {
    let mut out = String::from("# Your work\n");
    for (lane, items) in [("Initiatives", &work.initiatives), ("Tasks", &work.tasks)] {
        out.push_str(&format!("\n## {lane} ({})\n", items.len()));
        if items.is_empty() {
            out.push_str("(none)\n");
            continue;
        }
        for (state, heading) in STATES {
            let in_state: Vec<&WorkItem> = items.iter().filter(|i| i.state == state).collect();
            if in_state.is_empty() {
                continue;
            }
            out.push_str(&format!("\n### {heading}\n"));
            for item in in_state {
                out.push_str(&item_line(item));
            }
        }
    }
    out
}
