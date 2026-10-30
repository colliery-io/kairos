//! The notifications of a person (KAIROS-T-0381, COLLIERY-I-0614).
//!
//! # The events
//!
//! Each producer runs in the transaction of the change that it is about, so
//! a rolled back change makes no notification:
//!
//! - `hand_off`: a hand-off of the claim of a task ([`crate::task_claims::hand_off`])
//!   tells the person who gets the claim.
//! - `blocker_cleared`: an item that blocks an item (a `blocks` edge from
//!   the blocker to the dependant) goes into a done column, or is archived
//!   or purged, or the edge is removed (KAIROS-T-0568). The person who has
//!   the claim on the dependant task, or the owner of the dependant
//!   strategy or initiative, is told, when the dependant is not done. An
//!   item that was done already blocked nothing, so its archive, its purge
//!   or the removal of its edge tells nobody.
//! - `blocked_again`: a blocker leaves a done column, so it blocks again
//!   (KAIROS-T-0568). The person with the claim on each dependant that is
//!   not done is told, or the owner, or the person who created the
//!   dependant when it has neither.
//! - `cancelled`: a cancel of a task ([`crate::task_cancellations::cancel_task`])
//!   tells the person who created the task, with the reason.
//! - `blocker_cancelled`: a cancel of a task that blocks an item
//!   (KAIROS-T-0567). A cancelled blocker is unresolved, not done: it still
//!   blocks, and Kairos unblocks nothing. The person with the claim on each
//!   dependant that is not done (or its owner) and the person who created
//!   it are told, with the reason.
//! - `mention`: `@user_name` in the content of a new item, or a NEW mention
//!   in an update of the content (a note of `append_note` is an update). A
//!   mention is new when the content has more mentions of the name than
//!   before ([`kairos_core::notifications::new_mentions`]). A mention in
//!   code (a code span or a fenced block) is not a mention. A name that is
//!   not the user name of a person of the organization makes nothing.
//! - `p0_bug`: a bug with the priority P0 ([`kairos_core::notifications::is_p0_priority`])
//!   lands on a board with a team: its priority changes to P0, or it moves
//!   to the board. Each member of the team is told.
//! - `agent_key_revoked`: an organization admin revokes the agent key of a
//!   person ([`on_agent_key_revoked`]). The person is told the name and the
//!   prefix of the key.
//! - `claim_dropped`: a person leaves the organization, so each claim of
//!   the person ends ([`crate::task_claims::drop_claims_of_user`],
//!   KAIROS-T-0531). Each member of the team of the board of the task is
//!   told: the task is free.
//! - `claim_stale`: a claim had no write and no state change for some days
//!   ([`crate::task_claims::notify_stale_claims`], KAIROS-T-0531). The
//!   holder and the owner of the initiative of the task are told, once for
//!   each quiet period. Kairos is the actor.
//! - `proposal_decided`: a decision of an edge proposal
//!   ([`crate::proposals::confirm`], [`crate::proposals::reject`],
//!   KAIROS-T-0558). The proposer is told the state and the decider. An
//!   agent key proposes as its person, so the person is told.
//! - `key_expiring`: a key expires within 7 days
//!   ([`crate::api_keys::notify_expiring_keys`], KAIROS-T-0540). The person
//!   of an agent key is told, or the organization admins for a key of a
//!   service account. Once for each key, not for a revoked key and not for
//!   a key that a rotation replaced. Kairos is the actor.
//!
//! # Who gets a notification
//!
//! Only a person (`users.kind = 'human'`) who is a member of the
//! organization of the tenant. Never the actor of the change: a person is
//! not told what they did. Not a person who turned the kind off
//! (`notification_settings`). [`notify`] applies the 3 rules in its one
//! insert.
//!
//! The new notifications of one write send one event
//! `notification_created` to their persons only, with the list of the
//! persons ([`crate::events::emit_notification_event`], KAIROS-T-0632).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_query;
use diesel::sql_types::{Array, BigInt, Bool, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use kairos_core::notifications::{NotificationKind, is_p0_priority};
use uuid::Uuid;

use crate::events;

/// One notification as a person reads it: the code and the title of the
/// item now (`None` when the item is gone).
#[derive(Debug, Clone, PartialEq, Eq, QueryableByName)]
pub struct Notification {
    #[diesel(sql_type = SqlUuid)]
    pub id: Uuid,
    #[diesel(sql_type = Text)]
    pub kind: String,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub item_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    pub short_code: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub entity_type: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub title: Option<String>,
    #[diesel(sql_type = Text)]
    pub text: String,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub actor_id: Option<Uuid>,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub read_at: Option<DateTime<Utc>>,
    /// The agent key that made the notification (KAIROS-T-0737): a notice
    /// from an agent. `None`: a person, or Kairos.
    #[diesel(sql_type = Nullable<SqlUuid>)]
    pub agent_key_id: Option<Uuid>,
    /// The name of that agent key.
    #[diesel(sql_type = Nullable<Text>)]
    pub agent_key_name: Option<String>,
}

/// [`unread`]: the newest unread notifications, and whether there are more
/// (KAIROS-T-0612: no count, so the query stops at the limit).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Unread {
    pub newest: Vec<Notification>,
    pub more: bool,
}

#[derive(QueryableByName)]
struct Recipient {
    #[diesel(sql_type = SqlUuid)]
    user_id: Uuid,
}

/// Make one notification of `kind` for each person in `recipients` who may
/// get it (module docs: a person of the organization, not the actor, the
/// kind not turned off), and send each one the event. `short_code` is the
/// code of the item for the event. Returns the persons told. An `actor`
/// of [`Uuid::nil`] is Kairos itself (KAIROS-T-0384): the row has no actor.
pub fn notify(
    conn: &mut PgConnection,
    recipients: &[Uuid],
    kind: NotificationKind,
    item: Option<(Uuid, &str)>,
    text: &str,
    actor: Uuid,
) -> Result<Vec<Uuid>, DieselError> {
    notify_from(conn, recipients, kind, item, text, actor, None)
}

/// The agent key that makes a notification (KAIROS-T-0737).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FromKey {
    /// The key of the request.
    pub id: Uuid,
    /// The key, and each key that it replaced by a rotation. A person who
    /// muted one of them gets nothing.
    pub chain: Vec<Uuid>,
}

/// [`notify`], made by the agent key `from` (KAIROS-T-0737). The row names
/// the key. The person of the key (the actor) can get it: an agent can
/// tell its own person. A person who muted the key, or a key that it
/// replaced, gets nothing.
pub(crate) fn notify_from(
    conn: &mut PgConnection,
    recipients: &[Uuid],
    kind: NotificationKind,
    item: Option<(Uuid, &str)>,
    text: &str,
    actor: Uuid,
    from: Option<&FromKey>,
) -> Result<Vec<Uuid>, DieselError> {
    if recipients.is_empty() || text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let chain: Vec<Uuid> = from.map(|k| k.chain.clone()).unwrap_or_default();
    let told: Vec<Recipient> = sql_query(
        "INSERT INTO notifications (user_id, kind, item_id, text, actor_id, agent_key_id) \
         SELECT DISTINCT u.id, $2, $3, $4, NULLIF($5, '00000000-0000-0000-0000-000000000000'::uuid), $6 \
           FROM public.users u \
           JOIN public.organization_members m ON m.user_id = u.id \
           JOIN public.organizations o ON o.id = m.organization_id \
          WHERE u.id = ANY($1) \
            AND u.kind = 'human' \
            AND ($6::uuid IS NOT NULL OR u.id <> $5) \
            AND 'org_' || o.slug = current_schema() \
            AND NOT EXISTS (SELECT 1 FROM notification_settings s \
                             WHERE s.user_id = u.id AND s.kind = $2 AND NOT s.enabled) \
            AND NOT EXISTS (SELECT 1 FROM notice_mutes nm \
                             WHERE nm.user_id = u.id AND nm.api_key_id = ANY($7)) \
         RETURNING user_id",
    )
    .bind::<Array<SqlUuid>, _>(recipients)
    .bind::<Text, _>(kind.as_str())
    .bind::<Nullable<SqlUuid>, _>(item.map(|(id, _)| id))
    .bind::<Text, _>(text.trim())
    .bind::<SqlUuid, _>(actor)
    .bind::<Nullable<SqlUuid>, _>(from.map(|k| k.id))
    .bind::<Array<SqlUuid>, _>(&chain)
    .load(conn)?;
    let short_code = item.map_or("", |(_, code)| code);
    let mut users: Vec<Uuid> = told.into_iter().map(|r| r.user_id).collect();
    users.sort();
    // KAIROS-T-0632: one event with the list of the persons told.
    events::emit_notification_event(
        conn,
        events::NOTIFICATION_CREATED,
        &users,
        short_code,
        actor,
    )?;
    Ok(users)
}

/// The display name of a user, or the id when the user has no name.
pub(crate) fn name_of(conn: &mut PgConnection, user: Uuid) -> Result<String, DieselError> {
    use crate::schema::users;
    let name: Option<String> = users::table
        .filter(users::id.eq(user))
        .select(users::display_name)
        .first(conn)
        .optional()?;
    Ok(name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| user.to_string()))
}

/// `(short_code, title, created_by)` of a task, archived or not.
fn task_head(
    conn: &mut PgConnection,
    task_id: Uuid,
) -> Result<Option<(String, String, Uuid)>, DieselError> {
    use crate::schema::tasks;
    tasks::table
        .filter(tasks::id.eq(task_id))
        .select((tasks::short_code, tasks::title, tasks::created_by))
        .first(conn)
        .optional()
}

/// `agent_key_revoked` (KAIROS-T-0388): the organization admin `actor`
/// revoked the agent key `name` (`prefix`) of `owner`.
pub fn on_agent_key_revoked(
    conn: &mut PgConnection,
    owner: Uuid,
    name: &str,
    prefix: &str,
    actor: Uuid,
) -> Result<(), DieselError> {
    let text = format!(
        "{} revoked your agent key {name} ({prefix}). The key does not work now. \
         Make a new key on the Agent keys page.",
        name_of(conn, actor)?
    );
    notify(
        conn,
        &[owner],
        NotificationKind::AgentKeyRevoked,
        None,
        &text,
        actor,
    )?;
    Ok(())
}

/// `key_expiring` (KAIROS-T-0540): the key expires within 7 days. The
/// person of an agent key is told. For a key of a service account, each
/// organization admin is told. Kairos is the actor. The text names the key
/// and the command that rotates it.
pub fn on_key_expiring(
    conn: &mut PgConnection,
    key: &crate::api_keys::ExpiringKey,
) -> Result<(), DieselError> {
    let at = key.expires_at.format("%Y-%m-%d %H:%M UTC");
    let (name, prefix, id) = (&key.name, &key.prefix, key.id);
    let (recipients, text) = if key.is_agent_key() {
        (
            vec![key.user_id],
            format!(
                "Your agent key {name} ({prefix}) expires at {at}. Then it does not work. \
                 Rotate it before then: kairos keys rotate {id}, or Rotate on the Agent keys page."
            ),
        )
    } else {
        let admins: Vec<Recipient> = sql_query(
            "SELECT m.user_id FROM public.organization_members m \
               JOIN public.organizations o ON o.id = m.organization_id \
              WHERE m.role = 'admin' AND 'org_' || o.slug = current_schema()",
        )
        .load(conn)?;
        (
            admins.into_iter().map(|r| r.user_id).collect(),
            format!(
                "The key {name} ({prefix}) of the service account {} expires at {at}. Then it \
                 does not work. Rotate it before then: kairos keys rotate {id} \
                 --service-account {}",
                key.principal_name, key.user_id
            ),
        )
    };
    notify(
        conn,
        &recipients,
        NotificationKind::KeyExpiring,
        None,
        &text,
        Uuid::nil(),
    )?;
    Ok(())
}

/// `proposal_decided` (KAIROS-T-0558): `actor` confirmed or rejected the
/// edge proposal. The proposer is told, with the edge and the state. The
/// notification is on the source of the edge.
pub fn on_proposal_decided(
    conn: &mut PgConnection,
    proposal: &crate::proposals::EdgeProposal,
    actor: Uuid,
) -> Result<(), DieselError> {
    let code = |conn: &mut PgConnection, id: Uuid| -> Result<String, DieselError> {
        Ok(item_row(conn, id)?.map_or_else(|| id.to_string(), |row| row.short_code))
    };
    let source = code(conn, proposal.source_id)?;
    let target = code(conn, proposal.target_id)?;
    let outcome = if proposal.state == "confirmed" {
        "The edge exists now."
    } else {
        "Kairos made no edge."
    };
    let text = format!(
        "{} {} your proposal {source} -[{}]-> {target} ({}). {outcome}",
        name_of(conn, actor)?,
        proposal.state,
        proposal.relationship,
        proposal.id
    );
    notify(
        conn,
        &[proposal.proposed_by],
        NotificationKind::ProposalDecided,
        Some((proposal.source_id, &source)),
        &text,
        actor,
    )?;
    Ok(())
}

/// `hand_off`: `to_user` got the claim of the task from `actor`.
pub fn on_hand_off(
    conn: &mut PgConnection,
    task_id: Uuid,
    to_user: Uuid,
    actor: Uuid,
) -> Result<(), DieselError> {
    let Some((code, title, _)) = task_head(conn, task_id)? else {
        return Ok(());
    };
    let text = format!(
        "{} gave you the claim on {code}: {title}",
        name_of(conn, actor)?
    );
    notify(
        conn,
        &[to_user],
        NotificationKind::HandOff,
        Some((task_id, &code)),
        &text,
        actor,
    )?;
    Ok(())
}

/// `hand_off` of an owner (KAIROS-T-0411): `actor` made `to_user` the
/// owner of the strategy or the initiative `(id, code, title)`.
pub fn on_owner_hand_off(
    conn: &mut PgConnection,
    (item_id, code, title): (Uuid, &str, &str),
    to_user: Uuid,
    actor: Uuid,
) -> Result<(), DieselError> {
    let text = format!(
        "{} made you the owner of {code}: {title}",
        name_of(conn, actor)?
    );
    notify(
        conn,
        &[to_user],
        NotificationKind::HandOff,
        Some((item_id, code)),
        &text,
        actor,
    )?;
    Ok(())
}

/// `cancelled`: `actor` cancelled the task, with `reason`. The person who
/// created the task is told.
pub fn on_cancel(
    conn: &mut PgConnection,
    task_id: Uuid,
    reason: &str,
    actor: Uuid,
) -> Result<(), DieselError> {
    let Some((code, title, created_by)) = task_head(conn, task_id)? else {
        return Ok(());
    };
    let text = format!(
        "{} cancelled {code}, which you created: {title}. The reason: {}",
        name_of(conn, actor)?,
        reason.trim()
    );
    notify(
        conn,
        &[created_by],
        NotificationKind::Cancelled,
        Some((task_id, &code)),
        &text,
        actor,
    )?;
    Ok(())
}

/// `claim_dropped` (KAIROS-T-0531): the person `left` left the
/// organization, and their claim on the task ended. The members of the
/// team of the board of the task are told.
pub fn on_claim_dropped(
    conn: &mut PgConnection,
    task_id: Uuid,
    left: Uuid,
    actor: Uuid,
) -> Result<(), DieselError> {
    use crate::schema::{board_columns, boards, tasks, team_members};
    let Some((code, title, _)) = task_head(conn, task_id)? else {
        return Ok(());
    };
    let place: Option<(String, Option<Uuid>)> = tasks::table
        .inner_join(board_columns::table)
        .inner_join(boards::table)
        .filter(tasks::id.eq(task_id))
        .select((board_columns::name, boards::team_id))
        .first(conn)
        .optional()?;
    let Some((column, Some(team_id))) = place else {
        return Ok(());
    };
    let members: Vec<Uuid> = team_members::table
        .filter(team_members::team_id.eq(team_id))
        .filter(team_members::user_id.ne(left))
        .select(team_members::user_id)
        .load(conn)?;
    let text = format!(
        "{} left the organization, so the claim on {code} ended. The task is free in {column}: \
         {title}",
        name_of(conn, left)?
    );
    notify(
        conn,
        &members,
        NotificationKind::ClaimDropped,
        Some((task_id, &code)),
        &text,
        actor,
    )?;
    Ok(())
}

/// `claim_stale` (KAIROS-T-0531): the claim had no write since
/// `claim.last_write`. The holder is told, and the owner of the initiative
/// of the task when it is a different person. Kairos is the actor.
pub fn on_claim_stale(
    conn: &mut PgConnection,
    claim: &crate::task_claims::StaleClaim,
    now: DateTime<Utc>,
) -> Result<(), DieselError> {
    let days = (now - claim.last_write).num_days().max(1);
    let (code, title) = (&claim.short_code, &claim.title);
    let text = format!(
        "Your claim on {code} has no write and no state change for {days} days: {title}. \
         Write a note or the state, or release the task."
    );
    notify(
        conn,
        &[claim.user_id],
        NotificationKind::ClaimStale,
        Some((claim.task_id, code)),
        &text,
        Uuid::nil(),
    )?;
    if let Some(owner) = claim.owner_id.filter(|owner| *owner != claim.user_id) {
        let text = format!(
            "The claim of {} on {code} has no write and no state change for {days} days. \
             The task is part of an initiative that you own: {title}",
            name_of(conn, claim.user_id)?
        );
        notify(
            conn,
            &[owner],
            NotificationKind::ClaimStale,
            Some((claim.task_id, code)),
            &text,
            Uuid::nil(),
        )?;
    }
    Ok(())
}

#[derive(QueryableByName)]
struct ClaimantRow {
    #[diesel(sql_type = SqlUuid)]
    user_id: Uuid,
    #[diesel(sql_type = SqlUuid)]
    item_id: Uuid,
    #[diesel(sql_type = Text)]
    item_code: String,
}

/// Why a blocker no longer blocks: [`on_blockers_cleared`] and
/// [`on_blocks_removed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleared {
    /// The blocker went into a done column.
    Done,
    /// The blocker was archived. A blocker in a done column blocked nothing
    /// before the archive, so it tells nobody.
    Archived,
    /// The `blocks` edge was removed (KAIROS-T-0568).
    Unlinked,
    /// The blocker was purged (KAIROS-T-0568).
    Purged,
}

impl Cleared {
    fn why(self) -> &'static str {
        match self {
            Cleared::Done => "it is done",
            Cleared::Archived => "it is archived",
            Cleared::Unlinked => "the blocks link is removed",
            Cleared::Purged => "it is purged",
        }
    }
}

/// `blocker_cleared`: the items in `blockers` no longer block (`how` is
/// [`Cleared::Done`] or [`Cleared::Archived`]). Each item that one of them
/// blocks, and that is not in `blockers`, lost a blocker:
/// [`on_blocks_removed`].
pub fn on_blockers_cleared(
    conn: &mut PgConnection,
    blockers: &[Uuid],
    how: Cleared,
    actor: Uuid,
) -> Result<(), DieselError> {
    if blockers.is_empty() {
        return Ok(());
    }
    // A blocker that was done before its archive closed nothing now. A
    // cancelled blocker was open (KAIROS-T-0567): its archive closes it.
    let closed: Vec<Uuid> = match how {
        Cleared::Archived => blockers_open(conn, blockers)?,
        _ => blockers.to_vec(),
    };
    let freed = crate::dispatch::dependants_of(conn, &closed, blockers)?;
    on_blocks_removed(conn, &freed, how, actor)
}

/// The `blocks` edges of `freed` block nothing now: the blocker closed, or
/// the edge is gone (KAIROS-T-0568). KAIROS-T-0427: the dispatch rules that
/// fire on `unblocked`. `blocker_cleared`: for each live item of `freed`
/// that is not done, the person with the claim on the task, or the owner
/// of the strategy or the initiative, is told.
pub fn on_blocks_removed(
    conn: &mut PgConnection,
    freed: &[crate::dispatch::Freed],
    how: Cleared,
    actor: Uuid,
) -> Result<(), DieselError> {
    if freed.is_empty() {
        return Ok(());
    }
    crate::dispatch::on_dependants_freed(conn, freed, actor)?;
    let ids: Vec<Uuid> = freed.iter().map(|f| f.dependant).collect();
    let rows = claimants(conn, &ids, false)?;
    for f in freed {
        let Some(row) = rows.iter().find(|r| r.item_id == f.dependant) else {
            continue;
        };
        let text = format!(
            "{} does not block {} now: {}.",
            f.blocker_code,
            row.item_code,
            how.why()
        );
        notify(
            conn,
            &[row.user_id],
            NotificationKind::BlockerCleared,
            Some((row.item_id, &row.item_code)),
            &text,
            actor,
        )?;
    }
    Ok(())
}

/// The person who answers for each live item of `ids` that is not done:
/// the claim of a task, or the owner of a strategy or an initiative. With
/// `or_creator`, the person who created the item when nobody has the claim
/// or owns it. An item with nobody has no row.
fn claimants(
    conn: &mut PgConnection,
    ids: &[Uuid],
    or_creator: bool,
) -> Result<Vec<ClaimantRow>, DieselError> {
    sql_query(format!(
        "SELECT d.id AS item_id, d.short_code AS item_code, \
                COALESCE(c.user_id, o.user_id, CASE WHEN $2 THEN d.created_by END) AS user_id \
           FROM {items} d \
           JOIN board_columns dc ON dc.id = d.column_id AND NOT dc.is_done \
           LEFT JOIN task_claims c ON c.task_id = d.id \
           LEFT JOIN item_owners o ON o.item_id = d.id \
          WHERE d.id = ANY($1) AND d.deleted_at IS NULL \
            AND COALESCE(c.user_id, o.user_id, CASE WHEN $2 THEN d.created_by END) IS NOT NULL \
          ORDER BY d.short_code",
        items = crate::dispatch::WORKFLOW_ITEMS,
    ))
    .bind::<Array<SqlUuid>, _>(ids)
    .bind::<Bool, _>(or_creator)
    .load(conn)
}

/// Whether the item blocks the targets of its `blocks` edges now: a live
/// strategy, initiative or task that is not in a done column.
pub(crate) fn blocks_now(conn: &mut PgConnection, item_id: Uuid) -> Result<bool, DieselError> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Bool)]
        blocks: bool,
    }
    let row: Option<Row> = sql_query(format!(
        "SELECT {blocks} AS blocks FROM {items} p WHERE p.id = $1",
        blocks = crate::dispatch::blocks_now_sql("p"),
        items = crate::dispatch::WORKFLOW_ITEMS,
    ))
    .bind::<SqlUuid, _>(item_id)
    .get_result(conn)
    .optional()?;
    Ok(row.is_some_and(|r| r.blocks))
}

/// `blocked_again` (KAIROS-T-0568): the blocker left a done column, so it
/// blocks again. For each live item that it blocks and that is not done,
/// the person with the claim on the task (or the owner of the strategy or
/// the initiative) is told; the person who created the item when nobody
/// has the claim.
pub fn on_blocker_reopened(
    conn: &mut PgConnection,
    blocker: Uuid,
    actor: Uuid,
) -> Result<(), DieselError> {
    // KAIROS-T-0567: a cancelled blocker never stopped blocking.
    if crate::task_cancellations::mark_of(conn, blocker)?.is_some() {
        return Ok(());
    }
    let edges = crate::dispatch::dependants_of(conn, &[blocker], &[blocker])?;
    if edges.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = edges.iter().map(|f| f.dependant).collect();
    for row in claimants(conn, &ids, true)? {
        let Some(edge) = edges.iter().find(|f| f.dependant == row.item_id) else {
            continue;
        };
        let text = format!(
            "{} blocks {} again: it is not done now.",
            edge.blocker_code, row.item_code
        );
        notify(
            conn,
            &[row.user_id],
            NotificationKind::BlockedAgain,
            Some((row.item_id, &row.item_code)),
            &text,
            actor,
        )?;
    }
    Ok(())
}

/// `blocker_cancelled` (KAIROS-T-0567): `actor` cancelled the task
/// `blocker`, with `reason`. The task still blocks, so nothing is
/// unblocked. For each live item that it blocks and that is not done, the
/// person with the claim on the task (or the owner of the strategy or the
/// initiative) and the person who created the item are told.
pub fn on_blocker_cancelled(
    conn: &mut PgConnection,
    blocker: Uuid,
    reason: &str,
    actor: Uuid,
) -> Result<(), DieselError> {
    let edges = crate::dispatch::dependants_of(conn, &[blocker], &[blocker])?;
    if edges.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = edges.iter().map(|f| f.dependant).collect();
    let mut rows = claimants(conn, &ids, false)?;
    rows.extend(creators(conn, &ids)?);
    for edge in &edges {
        let mut told: Vec<Uuid> = rows
            .iter()
            .filter(|r| r.item_id == edge.dependant)
            .map(|r| r.user_id)
            .collect();
        told.sort();
        told.dedup();
        let Some(row) = rows.iter().find(|r| r.item_id == edge.dependant) else {
            continue;
        };
        let text = format!(
            "{} is cancelled, and it still blocks {}. Remove the blocks link, or reopen {}. \
             The reason: {}",
            edge.blocker_code,
            row.item_code,
            edge.blocker_code,
            reason.trim()
        );
        notify(
            conn,
            &told,
            NotificationKind::BlockerCancelled,
            Some((row.item_id, &row.item_code)),
            &text,
            actor,
        )?;
    }
    Ok(())
}

/// The person who created each live item of `ids` that is not done.
fn creators(conn: &mut PgConnection, ids: &[Uuid]) -> Result<Vec<ClaimantRow>, DieselError> {
    sql_query(format!(
        "SELECT d.id AS item_id, d.short_code AS item_code, d.created_by AS user_id \
           FROM {items} d \
           JOIN board_columns dc ON dc.id = d.column_id AND NOT dc.is_done \
          WHERE d.id = ANY($1) AND d.deleted_at IS NULL \
          ORDER BY d.short_code",
        items = crate::dispatch::WORKFLOW_ITEMS,
    ))
    .bind::<Array<SqlUuid>, _>(ids)
    .load(conn)
}

/// The items of `ids` that are open, live or archived: not in a done
/// column, or cancelled ([`crate::dispatch::open_sql`]).
fn blockers_open(conn: &mut PgConnection, ids: &[Uuid]) -> Result<Vec<Uuid>, DieselError> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = SqlUuid)]
        id: Uuid,
    }
    let rows: Vec<Row> = sql_query(format!(
        "SELECT p.id FROM {items} p WHERE p.id = ANY($1) AND {open}",
        items = crate::dispatch::WORKFLOW_ITEMS,
        open = crate::dispatch::open_sql("p"),
    ))
    .bind::<Array<SqlUuid>, _>(ids)
    .load(conn)?;
    Ok(rows.into_iter().map(|row| row.id).collect())
}

/// Whether the column is done. An unknown column is not.
pub(crate) fn column_is_done(
    conn: &mut PgConnection,
    column_id: Uuid,
) -> Result<bool, DieselError> {
    use crate::schema::board_columns;
    Ok(board_columns::table
        .filter(board_columns::id.eq(column_id))
        .select(board_columns::is_done)
        .first::<bool>(conn)
        .optional()?
        .unwrap_or(false))
}

/// `mention`: `names` (user names, lower case) were mentioned in the
/// content of the item by `actor`.
pub fn on_mentions(
    conn: &mut PgConnection,
    item_id: Uuid,
    names: &[String],
    actor: Uuid,
) -> Result<(), DieselError> {
    if names.is_empty() {
        return Ok(());
    }
    let users: Vec<Recipient> = sql_query(
        "SELECT id AS user_id FROM public.users WHERE lower(user_name) = ANY($1) ORDER BY id",
    )
    .bind::<Array<Text>, _>(names)
    .load(conn)?;
    if users.is_empty() {
        return Ok(());
    }
    let Some(ItemRow {
        short_code: code,
        title,
    }) = item_row(conn, item_id)?
    else {
        return Ok(());
    };
    let text = format!(
        "{} wrote your name in {code}: {title}",
        name_of(conn, actor)?
    );
    let ids: Vec<Uuid> = users.into_iter().map(|u| u.user_id).collect();
    notify(
        conn,
        &ids,
        NotificationKind::Mention,
        Some((item_id, &code)),
        &text,
        actor,
    )?;
    Ok(())
}

#[derive(QueryableByName)]
struct ItemRow {
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Text)]
    title: String,
}

/// The code and the title of an item of any type, archived or not.
fn item_row(conn: &mut PgConnection, item_id: Uuid) -> Result<Option<ItemRow>, DieselError> {
    sql_query("SELECT short_code, title FROM entity_directory WHERE id = $1")
        .bind::<SqlUuid, _>(item_id)
        .get_result(conn)
        .optional()
}

#[derive(QueryableByName)]
struct P0Row {
    #[diesel(sql_type = Text)]
    short_code: String,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Text)]
    board: String,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    team_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    priority: Option<String>,
}

fn p0_row(conn: &mut PgConnection, task_id: Uuid) -> Result<Option<P0Row>, DieselError> {
    sql_query(
        "SELECT t.short_code, t.title, b.name AS board, b.team_id, \
                (SELECT m.value FROM item_metadata m \
                   JOIN metadata_definitions d ON d.id = m.metadata_definition_id \
                  WHERE m.item_id = t.id AND d.slug = 'priority') AS priority \
           FROM tasks t JOIN boards b ON b.id = t.board_id \
          WHERE t.id = $1 AND t.deleted_at IS NULL AND t.task_type = 'bug'",
    )
    .bind::<SqlUuid, _>(task_id)
    .get_result(conn)
    .optional()
}

/// Whether the item is a live bug with the priority P0. Any other item is
/// not.
pub fn is_p0_bug(conn: &mut PgConnection, item_id: Uuid) -> Result<bool, DieselError> {
    Ok(p0_row(conn, item_id)?
        .and_then(|row| row.priority)
        .is_some_and(|p| is_p0_priority(&p)))
}

/// `p0_bug`: the item is a live P0 bug on its board now (it became P0, or
/// it moved to the board). Each member of the team of the board is told.
/// Nothing when the item is not a P0 bug, or the board has no team.
pub fn on_p0_bug(conn: &mut PgConnection, item_id: Uuid, actor: Uuid) -> Result<(), DieselError> {
    let Some(row) = p0_row(conn, item_id)? else {
        return Ok(());
    };
    if !row.priority.as_deref().is_some_and(is_p0_priority) {
        return Ok(());
    }
    let Some(team_id) = row.team_id else {
        return Ok(());
    };
    let members: Vec<Uuid> = {
        use crate::schema::team_members;
        team_members::table
            .filter(team_members::team_id.eq(team_id))
            .select(team_members::user_id)
            .load(conn)?
    };
    let text = format!(
        "{} is a critical bug on the board {}: {}",
        row.short_code, row.board, row.title
    );
    notify(
        conn,
        &members,
        NotificationKind::P0Bug,
        Some((item_id, &row.short_code)),
        &text,
        actor,
    )?;
    Ok(())
}

/// Who reads the notifications of a person (KAIROS-T-0550): the person, or
/// one agent key of the person. Each keeps its own read state. The person
/// reads `notifications.read_at`; a key reads its rows in
/// `notification_reads`. A notification is read for a key when the key, or
/// a key that it replaced by a rotation, read it. It is read for the key
/// also when the person read it before the first key of that chain was
/// made, so a new key does not start with the old history unread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reader {
    /// The person.
    pub user: Uuid,
    key: Option<KeyChain>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyChain {
    /// The key of the request: a mark writes its rows.
    id: Uuid,
    /// The key, and each key that it replaced.
    ids: Vec<Uuid>,
    /// When the first key of the chain was made.
    since: DateTime<Utc>,
}

#[derive(QueryableByName)]
struct ChainRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Timestamptz)]
    created_at: DateTime<Utc>,
}

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    total: i64,
}

impl Reader {
    /// The person reads.
    pub fn person(user: Uuid) -> Reader {
        Reader { user, key: None }
    }

    /// The reader of a request: the person, or the agent key of the person
    /// (`agent_key`). One query for a key: the chain of its rotations.
    pub fn of(
        conn: &mut PgConnection,
        user: Uuid,
        agent_key: Option<Uuid>,
    ) -> Result<Reader, DieselError> {
        let Some(key) = agent_key else {
            return Ok(Reader::person(user));
        };
        let rows: Vec<ChainRow> = sql_query(
            "WITH RECURSIVE chain(id, created_at) AS ( \
                 SELECT id, created_at FROM api_keys WHERE id = $1 AND user_id = $2 \
                 UNION \
                 SELECT k.id, k.created_at FROM api_keys k JOIN chain c ON k.replaced_by = c.id) \
             SELECT id, created_at FROM chain",
        )
        .bind::<SqlUuid, _>(key)
        .bind::<SqlUuid, _>(user)
        .load(conn)?;
        let since = rows
            .iter()
            .map(|r| r.created_at)
            .min()
            .unwrap_or_else(Utc::now);
        let mut ids: Vec<Uuid> = rows.into_iter().map(|r| r.id).collect();
        if !ids.contains(&key) {
            ids.push(key);
        }
        Ok(Reader {
            user,
            key: Some(KeyChain {
                id: key,
                ids,
                since,
            }),
        })
    }

    /// True when an agent key reads.
    pub fn is_key(&self) -> bool {
        self.key.is_some()
    }

    /// The agent key of the reader as the maker of a notification
    /// (KAIROS-T-0737): the key and the keys that it replaced. `None` for
    /// the person.
    pub fn from_key(&self) -> Option<FromKey> {
        self.key.as_ref().map(|k| FromKey {
            id: k.id,
            chain: k.ids.clone(),
        })
    }

    fn key_ids(&self) -> Vec<Uuid> {
        self.key.as_ref().map(|k| k.ids.clone()).unwrap_or_default()
    }

    fn since(&self) -> Option<DateTime<Utc>> {
        self.key.as_ref().map(|k| k.since)
    }
}

/// The notifications of the reader as the view `v`, with the time that
/// the reader read each one (`read_at`, NULL while unread). `$1` is the
/// person, `$2` the ids of the key chain (empty for the person), `$3` the
/// time of the first key of the chain (NULL for the person).
const READER_VIEW: &str = "WITH v AS ( \
     SELECT n.id, n.kind, n.item_id, n.text, n.actor_id, n.agent_key_id, n.created_at, \
            CASE WHEN $3::timestamptz IS NULL THEN n.read_at ELSE COALESCE( \
                (SELECT min(r.read_at) FROM notification_reads r \
                  WHERE r.notification_id = n.id AND r.api_key_id = ANY($2)), \
                CASE WHEN n.read_at < $3 THEN n.read_at END) END AS read_at \
       FROM notifications n WHERE n.user_id = $1)";

/// The columns of [`Notification`] from the view `v` of [`READER_VIEW`].
const SELECT_VIEW: &str = "SELECT v.id, v.kind, v.item_id, d.short_code, d.entity_type, \
     d.title, v.text, v.actor_id, v.created_at, v.read_at, v.agent_key_id, \
     k.name AS agent_key_name \
     FROM v LEFT JOIN entity_directory d ON d.id = v.item_id \
     LEFT JOIN api_keys k ON k.id = v.agent_key_id";

/// The newest `limit` unread notifications of the reader, and whether there
/// are more. One query.
///
/// KAIROS-T-0612: the query reads `limit + 1` rows and has no count, so it
/// stops at the limit; the join of `entity_directory` comes after the
/// limit, on those rows only. An agent rarely reads all its notifications,
/// and a count read each unread row on each MCP answer.
pub fn unread(conn: &mut PgConnection, reader: &Reader, limit: i64) -> Result<Unread, DieselError> {
    let mut newest: Vec<Notification> = sql_query(format!(
        "{READER_VIEW}, u AS ( \
             SELECT * FROM v WHERE v.read_at IS NULL \
              ORDER BY v.created_at DESC, v.id DESC LIMIT $4) \
         SELECT u.id, u.kind, u.item_id, d.short_code, d.entity_type, d.title, \
                u.text, u.actor_id, u.created_at, u.read_at, u.agent_key_id, \
                k.name AS agent_key_name \
           FROM u LEFT JOIN entity_directory d ON d.id = u.item_id \
           LEFT JOIN api_keys k ON k.id = u.agent_key_id \
          ORDER BY u.created_at DESC, u.id DESC"
    ))
    .bind::<SqlUuid, _>(reader.user)
    .bind::<Array<SqlUuid>, _>(reader.key_ids())
    .bind::<Nullable<Timestamptz>, _>(reader.since())
    .bind::<BigInt, _>(limit + 1)
    .load(conn)?;
    let more = newest.len() as i64 > limit;
    newest.truncate(limit.max(0) as usize);
    Ok(Unread { newest, more })
}

/// The notifications of the reader for a list: the unread ones (newest
/// first, at most `unread_limit`), then the newest `read_limit` read ones.
/// With `before` (the id of a notification of the person), the next page:
/// the unread ones older than it, and no read ones.
pub fn list(
    conn: &mut PgConnection,
    reader: &Reader,
    before: Option<Uuid>,
    unread_limit: i64,
    read_limit: i64,
) -> Result<Vec<Notification>, DieselError> {
    let mut out: Vec<Notification> = sql_query(format!(
        "{READER_VIEW} {SELECT_VIEW} WHERE v.read_at IS NULL \
           AND ($4::uuid IS NULL OR (v.created_at, v.id) < \
                (SELECT b.created_at, b.id FROM notifications b WHERE b.id = $4 AND b.user_id = $1)) \
          ORDER BY v.created_at DESC, v.id DESC LIMIT $5"
    ))
    .bind::<SqlUuid, _>(reader.user)
    .bind::<Array<SqlUuid>, _>(reader.key_ids())
    .bind::<Nullable<Timestamptz>, _>(reader.since())
    .bind::<Nullable<SqlUuid>, _>(before)
    .bind::<BigInt, _>(unread_limit)
    .load(conn)?;
    if before.is_none() {
        let read: Vec<Notification> = sql_query(format!(
            "{READER_VIEW} {SELECT_VIEW} WHERE v.read_at IS NOT NULL \
              ORDER BY v.read_at DESC, v.id DESC LIMIT $4"
        ))
        .bind::<SqlUuid, _>(reader.user)
        .bind::<Array<SqlUuid>, _>(reader.key_ids())
        .bind::<Nullable<Timestamptz>, _>(reader.since())
        .bind::<BigInt, _>(read_limit)
        .load(conn)?;
        out.extend(read);
    }
    Ok(out)
}

/// The next page of the read notifications of the reader (KAIROS-T-0584):
/// the ones read before `read_before`, in the order of [`list`] (the time
/// of the read, then the id), newest first, `limit` at most. `None`: the
/// reader did not read `read_before`, or it is not a notification of the
/// person. A page has no unread notification.
pub fn read_page(
    conn: &mut PgConnection,
    reader: &Reader,
    read_before: Uuid,
    limit: i64,
) -> Result<Option<Vec<Notification>>, DieselError> {
    #[derive(QueryableByName)]
    struct ReadAt {
        #[diesel(sql_type = Timestamptz)]
        read_at: DateTime<Utc>,
    }
    let cursor: Option<ReadAt> = sql_query(format!(
        "{READER_VIEW} SELECT v.read_at FROM v WHERE v.id = $4 AND v.read_at IS NOT NULL"
    ))
    .bind::<SqlUuid, _>(reader.user)
    .bind::<Array<SqlUuid>, _>(reader.key_ids())
    .bind::<Nullable<Timestamptz>, _>(reader.since())
    .bind::<SqlUuid, _>(read_before)
    .get_result(conn)
    .optional()?;
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let page: Vec<Notification> = sql_query(format!(
        "{READER_VIEW} {SELECT_VIEW} WHERE v.read_at IS NOT NULL \
           AND (v.read_at, v.id) < ($4, $5) \
          ORDER BY v.read_at DESC, v.id DESC LIMIT $6"
    ))
    .bind::<SqlUuid, _>(reader.user)
    .bind::<Array<SqlUuid>, _>(reader.key_ids())
    .bind::<Nullable<Timestamptz>, _>(reader.since())
    .bind::<Timestamptz, _>(cursor.read_at)
    .bind::<SqlUuid, _>(read_before)
    .bind::<BigInt, _>(limit)
    .load(conn)?;
    Ok(Some(page))
}

/// What [`mark_read`] marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark<'a> {
    /// These ids.
    Ids(&'a [Uuid]),
    /// This notification of the person, and each older one: the order of
    /// [`list`] (time, then id). A newer one stays unread.
    UpTo(Uuid),
}

/// Mark notifications read for the reader. An id of a different person,
/// or of a notification that the reader read already, changes nothing.
/// Returns the ids that changed. A read of the person sends the event
/// `notifications_read` to the person when one changed. A read of a key
/// sends none: the bell of the person does not change.
pub fn mark_read(
    conn: &mut PgConnection,
    reader: &Reader,
    mark: Mark<'_>,
) -> Result<Vec<Uuid>, DieselError> {
    let (ids, up_to): (Vec<Uuid>, Option<Uuid>) = match mark {
        Mark::Ids(ids) => (ids.to_vec(), None),
        Mark::UpTo(id) => (Vec::new(), Some(id)),
    };
    let rows: Vec<IdRow> = match &reader.key {
        None => sql_query(
            "UPDATE notifications n SET read_at = now() \
              WHERE n.user_id = $1 AND n.read_at IS NULL \
                AND (n.id = ANY($2) OR (n.created_at, n.id) <= \
                     (SELECT b.created_at, b.id FROM notifications b \
                       WHERE b.id = $3 AND b.user_id = $1)) \
             RETURNING n.id",
        )
        .bind::<SqlUuid, _>(reader.user)
        .bind::<Array<SqlUuid>, _>(ids)
        .bind::<Nullable<SqlUuid>, _>(up_to)
        .load(conn)?,
        Some(key) => sql_query(format!(
            "{READER_VIEW} INSERT INTO notification_reads (notification_id, api_key_id) \
             SELECT v.id, $6 FROM v WHERE v.read_at IS NULL \
                AND (v.id = ANY($4) OR (v.created_at, v.id) <= \
                     (SELECT b.created_at, b.id FROM notifications b \
                       WHERE b.id = $5 AND b.user_id = $1)) \
             ON CONFLICT DO NOTHING RETURNING notification_id AS id"
        ))
        .bind::<SqlUuid, _>(reader.user)
        .bind::<Array<SqlUuid>, _>(reader.key_ids())
        .bind::<Nullable<Timestamptz>, _>(reader.since())
        .bind::<Array<SqlUuid>, _>(ids)
        .bind::<Nullable<SqlUuid>, _>(up_to)
        .bind::<SqlUuid, _>(key.id)
        .load(conn)?,
    };
    let changed: Vec<Uuid> = rows.into_iter().map(|r| r.id).collect();
    if !changed.is_empty() && !reader.is_key() {
        events::emit_notification_event(
            conn,
            events::NOTIFICATIONS_READ,
            &[reader.user],
            "",
            reader.user,
        )?;
    }
    Ok(changed)
}

/// The ids in `ids` that are not notifications of `user`: unknown ids, and
/// the ids of the notifications of a different person. One query.
pub fn not_of(conn: &mut PgConnection, user: Uuid, ids: &[Uuid]) -> Result<Vec<Uuid>, DieselError> {
    use crate::schema::notifications::dsl;
    let found: Vec<Uuid> = dsl::notifications
        .filter(dsl::user_id.eq(user))
        .filter(dsl::id.eq_any(ids))
        .select(dsl::id)
        .load(conn)?;
    Ok(ids
        .iter()
        .copied()
        .filter(|id| !found.contains(id))
        .collect())
}

/// The count of the unread notifications of the reader.
pub fn unread_count(conn: &mut PgConnection, reader: &Reader) -> Result<i64, DieselError> {
    let row: CountRow = sql_query(format!(
        "{READER_VIEW} SELECT count(*) AS total FROM v WHERE v.read_at IS NULL"
    ))
    .bind::<SqlUuid, _>(reader.user)
    .bind::<Array<SqlUuid>, _>(reader.key_ids())
    .bind::<Nullable<Timestamptz>, _>(reader.since())
    .get_result(conn)?;
    Ok(row.total)
}

/// The settings of `user`: each kind, and whether it is on.
pub fn settings(
    conn: &mut PgConnection,
    user: Uuid,
) -> Result<BTreeMap<NotificationKind, bool>, DieselError> {
    use crate::schema::notification_settings::dsl;
    let stored: Vec<(String, bool)> = dsl::notification_settings
        .filter(dsl::user_id.eq(user))
        .select((dsl::kind, dsl::enabled))
        .load(conn)?;
    let mut out: BTreeMap<NotificationKind, bool> = NotificationKind::ALL
        .into_iter()
        .map(|k| (k, true))
        .collect();
    for (kind, enabled) in stored {
        if let Some(kind) = NotificationKind::parse(&kind) {
            out.insert(kind, enabled);
        }
    }
    Ok(out)
}

/// Turn kinds on or off for `user`. A kind that is not in `changes` does
/// not change.
pub fn set_settings(
    conn: &mut PgConnection,
    user: Uuid,
    changes: &[(NotificationKind, bool)],
) -> Result<BTreeMap<NotificationKind, bool>, DieselError> {
    use crate::schema::notification_settings::dsl;
    conn.transaction(|conn| {
        for (kind, enabled) in changes {
            diesel::insert_into(dsl::notification_settings)
                .values((
                    dsl::user_id.eq(user),
                    dsl::kind.eq(kind.as_str()),
                    dsl::enabled.eq(*enabled),
                ))
                .on_conflict((dsl::user_id, dsl::kind))
                .do_update()
                .set(dsl::enabled.eq(*enabled))
                .execute(conn)?;
        }
        settings(conn, user)
    })
}

/// Delete at most `limit` notifications that were read before `cutoff`
/// (the retention pass, which calls it until a page deletes fewer than
/// `limit`, KAIROS-T-0638). Returns the count.
pub fn delete_read_before(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<usize, DieselError> {
    sql_query(
        "DELETE FROM notifications WHERE id IN ( \
             SELECT id FROM notifications WHERE read_at < $1 LIMIT $2)",
    )
    .bind::<Timestamptz, _>(cutoff)
    .bind::<BigInt, _>(limit)
    .execute(conn)
}

/// Delete at most `limit` notifications that nobody read and that were
/// made before `cutoff` (the retention pass, KAIROS-T-0638). Returns the
/// count.
pub fn delete_unread_before(
    conn: &mut PgConnection,
    cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<usize, DieselError> {
    sql_query(
        "DELETE FROM notifications WHERE id IN ( \
             SELECT id FROM notifications WHERE read_at IS NULL AND created_at < $1 LIMIT $2)",
    )
    .bind::<Timestamptz, _>(cutoff)
    .bind::<BigInt, _>(limit)
    .execute(conn)
}

/// Delete the notifications about an item (a purge of the item).
pub(crate) fn delete_for_item(
    conn: &mut PgConnection,
    item_id: Uuid,
) -> Result<usize, DieselError> {
    use crate::schema::notifications::dsl;
    diesel::delete(dsl::notifications.filter(dsl::item_id.eq(item_id))).execute(conn)
}
