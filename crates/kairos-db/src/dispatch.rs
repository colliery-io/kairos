//! The agent dispatch rules of a board (KAIROS-T-0385, COLLIERY-I-0614).
//!
//! A rule names a column of a board, a filter, and a webhook target of the
//! board ([`crate::webhooks`]). When a task enters the column and matches
//! the filter, Kairos queues one signed delivery of the event [`EVENT`] to
//! the target. Kairos does not start a session itself: the receiver does.
//!
//! The filter has 3 optional parts. Each part that is set must match:
//!
//! - the task type (`task`, `bug`, `tech_debt`, `support`);
//! - a metadata value: the slug of a metadata definition and a value; the
//!   value of the task matches with no case and no space at the ends;
//! - the repository of the task.
//!
//! A task enters a column by a transition ([`crate::boards::transition_task`]),
//! by a move to the board ([`crate::boards::move_task`], into the entry
//! column), or by a cancel ([`crate::task_cancellations::cancel_task`], into
//! the done column). [`on_task_entered_column`] runs in the transaction of
//! the move, so a move that is rolled back sends nothing, and each move
//! queues one delivery for each rule that matches.
//!
//! A rule with the trigger [`TRIGGER_UNBLOCKED`] (KAIROS-T-0427) fires when
//! an item loses its last open blocker ([`on_dependants_freed`]): the event
//! [`EVENT_UNBLOCKED`]. The blocker is done or archived
//! ([`on_blockers_closed`]), or its `blocks` edge is gone: an unlink or a
//! purge (KAIROS-T-0568). A cancelled blocker stays open (KAIROS-T-0567):
//! its cancel fires nothing, and it holds the item until a person removes
//! the edge, archives it or reopens it. The item can be a strategy, an initiative or a
//! task. The rule can name a column that the item must be in. An item with
//! two blockers fires once, when the second closes. A blocker that opens
//! again blocks the item again, so its next close fires again.
//!
//! The connection must be pinned to a tenant schema. This module does not
//! check rights: the caller does.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Array, Bool, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use uuid::Uuid;

/// The event of a task that enters the column of a rule.
pub const EVENT: &str = "task.entered_column";

/// The event of a task whose last open blocker closed (KAIROS-T-0427).
pub const EVENT_UNBLOCKED: &str = "task.unblocked";

/// The trigger of a rule that fires when a task enters its column.
pub const TRIGGER_ENTERED_COLUMN: &str = "entered_column";

/// The trigger of a rule that fires when a task is unblocked.
pub const TRIGGER_UNBLOCKED: &str = "unblocked";

/// The triggers that a rule can name.
pub const TRIGGERS: [&str; 2] = [TRIGGER_ENTERED_COLUMN, TRIGGER_UNBLOCKED];

/// The task types that a rule can name.
pub const TASK_TYPES: [&str; 4] = ["task", "bug", "tech_debt", "support"];

/// A dispatch rule, with the names of its column, target and repository.
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct Rule {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    pub board_id: Uuid,
    #[diesel(sql_type = Text)]
    pub trigger: String,
    /// `None`: an `unblocked` rule with no column.
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub column_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    pub column_name: Option<String>,
    #[diesel(sql_type = SqlUuid)]
    pub target_id: Uuid,
    #[diesel(sql_type = Text)]
    pub target_name: String,
    #[diesel(sql_type = Nullable<Text>)]
    pub task_type: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub metadata_slug: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub metadata_value: Option<String>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub repository_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    pub repository_slug: Option<String>,
    #[diesel(sql_type = SqlUuid)]
    pub created_by: Uuid,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
}

const RULE_SELECT: &str = "SELECT r.id, r.board_id, r.trigger, r.column_id, c.name AS column_name, \
     r.target_id, t.name AS target_name, r.task_type, r.metadata_slug, r.metadata_value, \
     r.repository_id, p.slug AS repository_slug, r.created_by, r.created_at \
     FROM dispatch_rules r \
     LEFT JOIN board_columns c ON c.id = r.column_id \
     JOIN webhook_targets t ON t.id = r.target_id \
     LEFT JOIN repositories p ON p.id = r.repository_id";

/// A rule to add. The caller checked that the column and the target are of
/// the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRule {
    pub board_id: Uuid,
    /// [`TRIGGER_ENTERED_COLUMN`] or [`TRIGGER_UNBLOCKED`].
    pub trigger: String,
    /// Required for [`TRIGGER_ENTERED_COLUMN`].
    pub column_id: Option<Uuid>,
    pub target_id: Uuid,
    pub task_type: Option<String>,
    pub metadata_slug: Option<String>,
    pub metadata_value: Option<String>,
    pub repository_id: Option<Uuid>,
    pub created_by: Uuid,
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

/// Add a rule.
pub fn create_rule(conn: &mut PgConnection, rule: &NewRule) -> Result<Rule, DieselError> {
    let row: IdRow = sql_query(
        "INSERT INTO dispatch_rules (board_id, column_id, target_id, task_type, metadata_slug, \
             metadata_value, repository_id, created_by, trigger) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
    )
    .bind::<SqlUuid, _>(rule.board_id)
    .bind::<Nullable<SqlUuid>, _>(rule.column_id)
    .bind::<SqlUuid, _>(rule.target_id)
    .bind::<Nullable<Text>, _>(rule.task_type.as_deref())
    .bind::<Nullable<Text>, _>(rule.metadata_slug.as_deref())
    .bind::<Nullable<Text>, _>(rule.metadata_value.as_deref())
    .bind::<Nullable<SqlUuid>, _>(rule.repository_id)
    .bind::<SqlUuid, _>(rule.created_by)
    .bind::<Text, _>(&rule.trigger)
    .get_result(conn)?;
    get_rule(conn, row.id)?.ok_or(DieselError::NotFound)
}

/// The rule `id`.
pub fn get_rule(conn: &mut PgConnection, id: Uuid) -> Result<Option<Rule>, DieselError> {
    sql_query(format!("{RULE_SELECT} WHERE r.id = $1"))
        .bind::<SqlUuid, _>(id)
        .get_result(conn)
        .optional()
}

/// The rules of a board, oldest first.
pub fn list_rules(conn: &mut PgConnection, board_id: Uuid) -> Result<Vec<Rule>, DieselError> {
    sql_query(format!(
        "{RULE_SELECT} WHERE r.board_id = $1 ORDER BY r.created_at, r.id"
    ))
    .bind::<SqlUuid, _>(board_id)
    .load(conn)
}

/// Remove the rule `id` of the board `board_id`. False: the board has no
/// such rule.
pub fn delete_rule(conn: &mut PgConnection, board_id: Uuid, id: Uuid) -> Result<bool, DieselError> {
    let n = sql_query("DELETE FROM dispatch_rules WHERE id = $1 AND board_id = $2")
        .bind::<SqlUuid, _>(id)
        .bind::<SqlUuid, _>(board_id)
        .execute(conn)?;
    Ok(n == 1)
}

/// Whether `user` may read the dispatch rules and the delivery log of a
/// board: an admin of the organization `org_slug`, a person with a grant on
/// the board, a member of the team of the board, or a person with
/// `manage_structure` on the board or at the organization.
pub fn may_read(
    conn: &mut PgConnection,
    org_slug: &str,
    board_id: Uuid,
    user: Uuid,
) -> Result<bool, DieselError> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Bool)]
        allowed: bool,
    }
    let row: Row = sql_query(
        "SELECT ( \
             EXISTS (SELECT 1 FROM public.organization_members m \
                       JOIN public.organizations o ON o.id = m.organization_id \
                      WHERE o.slug = $1 AND m.user_id = $3 AND m.role = 'admin') \
             OR EXISTS (SELECT 1 FROM board_member_capabilities \
                         WHERE board_id = $2 AND user_id = $3) \
             OR EXISTS (SELECT 1 FROM boards b JOIN team_members tm ON tm.team_id = b.team_id \
                         WHERE b.id = $2 AND tm.user_id = $3) \
             OR EXISTS (SELECT 1 FROM structure_grants \
                         WHERE user_id = $3 AND (board_id IS NULL OR board_id = $2)) \
         ) AS allowed",
    )
    .bind::<Text, _>(org_slug)
    .bind::<SqlUuid, _>(board_id)
    .bind::<SqlUuid, _>(user)
    .get_result(conn)?;
    Ok(row.allowed)
}

/// The facts of an item for the payload: a task, or (for
/// [`EVENT_UNBLOCKED`]) a strategy or an initiative (KAIROS-T-0568).
#[derive(QueryableByName)]
struct TaskFacts {
    #[diesel(sql_type = Text)]
    entity_type: String,
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Nullable<Text>)]
    task_type: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    work_class: Option<String>,
    #[diesel(sql_type = SqlUuid)]
    board_id: Uuid,
    #[diesel(sql_type = Text)]
    board_slug: String,
    #[diesel(sql_type = Text)]
    board_name: String,
    #[diesel(sql_type = Bool)]
    agents_decide: bool,
    #[diesel(sql_type = Text)]
    column_name: String,
    #[diesel(sql_type = Nullable<Text>)]
    repository_slug: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    repository_full_name: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    repository_forge: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    repository_url: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    actor_name: Option<String>,
}

#[derive(QueryableByName)]
struct Match {
    #[diesel(sql_type = SqlUuid)]
    rule_id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    target_id: Uuid,
}

/// A task entered the column `column_id`, by `actor`: queue one delivery
/// for each rule of the column that the task matches. Returns the
/// deliveries.
pub fn on_task_entered_column(
    conn: &mut PgConnection,
    task_id: Uuid,
    column_id: Uuid,
    actor: Uuid,
) -> Result<Vec<crate::webhooks::Delivery>, DieselError> {
    let matches: Vec<Match> = sql_query(
        "SELECT r.id AS rule_id, r.target_id FROM dispatch_rules r \
           JOIN tasks t ON t.id = $1 \
          WHERE r.trigger = 'entered_column' \
            AND r.board_id = t.board_id AND r.column_id = $2 AND t.deleted_at IS NULL \
            AND (r.task_type IS NULL OR r.task_type = t.task_type) \
            AND (r.repository_id IS NULL OR r.repository_id = t.repository_id) \
            AND (r.metadata_slug IS NULL OR EXISTS ( \
                  SELECT 1 FROM item_metadata m \
                    JOIN metadata_definitions d ON d.id = m.metadata_definition_id \
                   WHERE m.item_id = t.id AND d.slug = r.metadata_slug \
                     AND lower(btrim(m.value)) = lower(btrim(r.metadata_value)))) \
          ORDER BY r.created_at, r.id",
    )
    .bind::<SqlUuid, _>(task_id)
    .bind::<SqlUuid, _>(column_id)
    .load(conn)?;
    queue(conn, task_id, column_id, actor, &matches, EVENT, None)
}

/// Queue one delivery of `event` for each of `matches`, for the item
/// `task_id` (a task, or for [`EVENT_UNBLOCKED`] any workflow item) in the
/// column `column_id`. `blocker` is the code of the
/// blocker that closed, for [`EVENT_UNBLOCKED`].
fn queue(
    conn: &mut PgConnection,
    task_id: Uuid,
    column_id: Uuid,
    actor: Uuid,
    matches: &[Match],
    event: &str,
    blocker: Option<&str>,
) -> Result<Vec<crate::webhooks::Delivery>, DieselError> {
    if matches.is_empty() {
        return Ok(Vec::new());
    }
    let facts: TaskFacts = sql_query(format!(
        "SELECT t.entity_type, t.short_code, t.title, t.task_type, t.work_class, \
                b.id AS board_id, b.slug AS board_slug, b.name AS board_name, COALESCE(a.agents_decide, false) AS agents_decide, \
                c.name AS column_name, p.slug AS repository_slug, \
                p.repo_full_name AS repository_full_name, p.forge AS repository_forge, \
                p.repo_url AS repository_url, \
                (SELECT COALESCE(NULLIF(btrim(u.display_name), ''), u.email) \
                   FROM public.users u WHERE u.id = $3) AS actor_name \
           FROM {WORKFLOW_ITEMS} t \
           JOIN boards b ON b.id = t.board_id \
           LEFT JOIN board_agent_triage a ON a.board_id = b.id \
           JOIN board_columns c ON c.id = $2 \
           LEFT JOIN repositories p ON p.id = t.repository_id AND p.deleted_at IS NULL \
          WHERE t.id = $1"
    ))
    .bind::<SqlUuid, _>(task_id)
    .bind::<SqlUuid, _>(column_id)
    .bind::<SqlUuid, _>(actor)
    .get_result(conn)?;
    let repository = match (
        &facts.repository_slug,
        &facts.repository_full_name,
        &facts.repository_forge,
    ) {
        (Some(slug), Some(full_name), Some(forge)) => serde_json::json!({
            "slug": slug,
            "full_name": full_name,
            "forge": forge,
            "url": facts.repository_url,
        }),
        _ => serde_json::Value::Null,
    };
    let mut deliveries = Vec::with_capacity(matches.len());
    for m in matches {
        let mut data = serde_json::json!({
            // KAIROS-T-0568: an `unblocked` item can be a strategy or an
            // initiative; its `task_type` and `work_class` are null.
            "entity_type": facts.entity_type,
            "code": facts.short_code,
            "title": facts.title,
            "task_type": facts.task_type,
            // KAIROS-T-0429: the lane, so a runner can triage support work.
            "work_class": facts.work_class,
            // The server makes the link absolute with KAIROS_PUBLIC_URL.
            "link": format!("/items/{}", facts.short_code),
            "board": {
                "id": facts.board_id,
                "slug": facts.board_slug,
                "name": facts.board_name,
                // KAIROS-T-0456: whether an agent may decide the triage of
                // intake here, so a runner knows before it tries.
                "agents_decide": facts.agents_decide,
            },
            "column": { "id": column_id, "name": facts.column_name },
            "repository": repository,
            "actor": { "id": actor, "name": facts.actor_name },
            "rule_id": m.rule_id,
        });
        if let Some(code) = blocker {
            data["blocker"] = serde_json::Value::String(code.to_string());
        }
        if let Some(delivery) = crate::webhooks::queue_delivery(
            conn,
            m.target_id,
            event,
            Some(task_id),
            Some(m.rule_id),
            &data,
        )? {
            deliveries.push(delivery);
        }
    }
    Ok(deliveries)
}

/// The items that a `blocks` edge joins (KAIROS-T-0568): strategies,
/// initiatives and tasks, with the columns of the dispatch and of the
/// notifications. `task_type`, `work_class` and `repository_id` are null
/// for a strategy and an initiative. Use it as a table: `{WORKFLOW_ITEMS} d`.
pub(crate) const WORKFLOW_ITEMS: &str = "(SELECT id, 'task'::text AS entity_type, short_code, \
     title, board_id, column_id, deleted_at, created_by, task_type, work_class::text AS work_class, \
     repository_id FROM tasks \
     UNION ALL SELECT id, 'initiative'::text, short_code, title, board_id, column_id, deleted_at, \
     created_by, NULL::text, NULL::text, NULL::uuid FROM initiatives \
     UNION ALL SELECT id, 'strategy'::text, short_code, title, board_id, column_id, deleted_at, \
     created_by, NULL::text, NULL::text, NULL::uuid FROM strategies)";

/// The SQL condition that the item `alias` of [`WORKFLOW_ITEMS`] blocks
/// the targets of its `blocks` edges now: it is live, and it is open
/// ([`open_sql`]).
pub(crate) fn blocks_now_sql(alias: &str) -> String {
    format!("({alias}.deleted_at IS NULL AND {})", open_sql(alias))
}

/// The SQL condition that the item `alias` of [`WORKFLOW_ITEMS`] is open,
/// live or archived: it is not in a done column, or it is a cancelled
/// task. KAIROS-T-0567: a cancelled blocker is unresolved, not done. The
/// work it stood for will not be done, so a person must decide: remove
/// the `blocks` link, archive the blocker, or reopen it.
pub(crate) fn open_sql(alias: &str) -> String {
    format!(
        "(NOT EXISTS (SELECT 1 FROM board_columns {alias}_c \
                       WHERE {alias}_c.id = {alias}.column_id AND {alias}_c.is_done) \
          OR EXISTS (SELECT 1 FROM task_cancellations {alias}_k \
                      WHERE {alias}_k.task_id = {alias}.id))"
    )
}

/// A dependant that a blocker no longer blocks: the target of a `blocks`
/// edge, and the code of the blocker (KAIROS-T-0568).
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct Freed {
    #[diesel(sql_type = SqlUuid)]
    pub dependant: Uuid,
    #[diesel(sql_type = Text)]
    pub blocker_code: String,
}

/// The targets of the `blocks` edges from `blockers`, with the code of the
/// blocker, that are not in `skip`. Ordered by target, then by blocker
/// code.
pub(crate) fn dependants_of(
    conn: &mut PgConnection,
    blockers: &[Uuid],
    skip: &[Uuid],
) -> Result<Vec<Freed>, DieselError> {
    if blockers.is_empty() {
        return Ok(Vec::new());
    }
    sql_query(
        "SELECT r.target_id AS dependant, b.short_code AS blocker_code \
           FROM item_relationships r \
           JOIN entity_directory b ON b.id = r.source_id \
          WHERE r.relationship = 'blocks' \
            AND r.source_id = ANY($1) \
            AND NOT (r.target_id = ANY($2)) \
          ORDER BY r.target_id, b.short_code",
    )
    .bind::<Array<SqlUuid>, _>(blockers)
    .bind::<Array<SqlUuid>, _>(skip)
    .load(conn)
}

#[derive(QueryableByName)]
struct Unblocked {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    column_id: Uuid,
}

/// The items in `blockers` closed: done or archived. For each live item
/// that one of them blocks, that is not done, that is not in `blockers`,
/// and that has no other open blocker now, queue one delivery of
/// [`EVENT_UNBLOCKED`] for each `unblocked` rule of its board that it
/// matches (KAIROS-T-0427). Runs in the transaction of the change, after
/// the blockers changed.
pub fn on_blockers_closed(
    conn: &mut PgConnection,
    blockers: &[Uuid],
    actor: Uuid,
) -> Result<Vec<crate::webhooks::Delivery>, DieselError> {
    let freed = dependants_of(conn, blockers, blockers)?;
    on_dependants_freed(conn, &freed, actor)
}

/// Each item of `freed` lost a blocker: the blocker closed, or its edge is
/// gone (an unlink, a purge: KAIROS-T-0568). For each item that is live,
/// that is not done, and that has no open blocker now, queue one delivery
/// of [`EVENT_UNBLOCKED`] for each `unblocked` rule of its board that it
/// matches. The item can be a strategy, an initiative or a task. Its
/// delivery names the first blocker code of `freed` for it.
pub fn on_dependants_freed(
    conn: &mut PgConnection,
    freed: &[Freed],
    actor: Uuid,
) -> Result<Vec<crate::webhooks::Delivery>, DieselError> {
    if freed.is_empty() {
        return Ok(Vec::new());
    }
    let mut ids: Vec<Uuid> = freed.iter().map(|f| f.dependant).collect();
    ids.sort();
    ids.dedup();
    // KAIROS-T-0457: lock the dependants, in one order. Two blockers of one
    // item closed in two transactions at one time each saw the other still
    // open, and neither queued the event. The second transaction now waits
    // here for the first to commit, and its check below (a new statement, a
    // new snapshot) sees the blocker closed. KAIROS-T-0568: the 3 tables,
    // each in the order of the ids, always in this order of the tables.
    for table in ["strategies", "initiatives", "tasks"] {
        sql_query(format!(
            "SELECT id FROM {table} WHERE deleted_at IS NULL AND id = ANY($1) \
              ORDER BY id FOR UPDATE"
        ))
        .bind::<Array<SqlUuid>, _>(&ids)
        .execute(conn)?;
    }
    let unblocked: Vec<Unblocked> = sql_query(format!(
        "SELECT d.id, d.column_id FROM {WORKFLOW_ITEMS} d \
           JOIN board_columns dc ON dc.id = d.column_id AND NOT dc.is_done \
          WHERE d.id = ANY($1) AND d.deleted_at IS NULL \
            AND NOT EXISTS ( \
                 SELECT 1 FROM item_relationships o \
                   JOIN {WORKFLOW_ITEMS} p ON p.id = o.source_id \
                  WHERE o.relationship = 'blocks' AND o.target_id = d.id \
                    AND {blocks}) \
          ORDER BY d.id",
        blocks = blocks_now_sql("p"),
    ))
    .bind::<Array<SqlUuid>, _>(&ids)
    .load(conn)?;
    let mut deliveries = Vec::new();
    for item in unblocked {
        let Some(blocker) = freed
            .iter()
            .filter(|f| f.dependant == item.id)
            .map(|f| f.blocker_code.as_str())
            .min()
        else {
            continue;
        };
        let matches: Vec<Match> = sql_query(format!(
            "SELECT r.id AS rule_id, r.target_id FROM dispatch_rules r \
               JOIN {WORKFLOW_ITEMS} t ON t.id = $1 \
              WHERE r.trigger = 'unblocked' AND r.board_id = t.board_id \
                AND (r.column_id IS NULL OR r.column_id = t.column_id) \
                AND (r.task_type IS NULL OR r.task_type = t.task_type) \
                AND (r.repository_id IS NULL OR r.repository_id = t.repository_id) \
                AND (r.metadata_slug IS NULL OR EXISTS ( \
                      SELECT 1 FROM item_metadata m \
                        JOIN metadata_definitions d ON d.id = m.metadata_definition_id \
                       WHERE m.item_id = t.id AND d.slug = r.metadata_slug \
                         AND lower(btrim(m.value)) = lower(btrim(r.metadata_value)))) \
              ORDER BY r.created_at, r.id"
        ))
        .bind::<SqlUuid, _>(item.id)
        .load(conn)?;
        deliveries.extend(queue(
            conn,
            item.id,
            item.column_id,
            actor,
            &matches,
            EVENT_UNBLOCKED,
            Some(blocker),
        )?);
    }
    Ok(deliveries)
}
