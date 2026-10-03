//! The read token of a repository (COLLIERY-T-3105, COLLIERY-I-0264): the
//! credential that the builder of the base code index gives to git to fetch
//! a private repository.
//!
//! This module stores and reads the ENCRYPTED form only. The encryption and
//! the key (`KAIROS_SECRETS_KEY`) are in the server; nothing here sees the
//! token. One row for each repository (`repository_credentials`).
//!
//! - [`put`] writes the row (a new one, or a replacement), and clears the
//!   result of the last access check, because that result was for the old
//!   token.
//! - [`statuses`] gives what a read shows: whether a token is set, who set
//!   it and when, and the last check. It never gives the ciphertext.
//! - [`load`] gives the ciphertext, for the server to decrypt.
//! - [`delete`] removes the row.
//!
//! Each write of a token writes an activity row with the actor. The row
//! holds the slug of the repository, never the token.
//!
//! The connection must be pinned to a tenant schema. This module does not
//! check rights: the caller does.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use crate::models::enums::ActivityAction;
use crate::models::graph::NewActivityLogEntry;
use crate::schema::repository_credentials;

/// The encrypted token of a repository, to write.
#[derive(Clone, PartialEq, Eq, Insertable)]
#[diesel(table_name = repository_credentials)]
pub struct NewCredential {
    pub repository_id: Uuid,
    /// The AES-256-GCM ciphertext with its tag.
    pub ciphertext: Vec<u8>,
    /// 12 random bytes.
    pub nonce: Vec<u8>,
    /// The fingerprint of the key that encrypted the token.
    pub key_id: String,
    pub set_by: Uuid,
}

impl std::fmt::Debug for NewCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewCredential")
            .field("repository_id", &self.repository_id)
            .field("key_id", &self.key_id)
            .field("set_by", &self.set_by)
            .finish_non_exhaustive()
    }
}

/// The stored, encrypted token of a repository.
#[derive(Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = repository_credentials)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct StoredCredential {
    pub repository_id: Uuid,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub key_id: String,
}

impl std::fmt::Debug for StoredCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredCredential")
            .field("repository_id", &self.repository_id)
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

/// What a read of the credential of a repository shows. It has no
/// ciphertext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialStatus {
    pub repository_id: Uuid,
    pub set_by: Uuid,
    /// The display name of `set_by`, else the email. `None` when the user
    /// is not in `public.users`.
    pub set_by_name: Option<String>,
    pub set_at: DateTime<Utc>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_check_ok: Option<bool>,
    pub last_check_error: Option<String>,
}

/// What [`put`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    /// The repository had no token.
    Set,
    /// The repository had a token, and the new one replaced it.
    Replaced,
}

/// Write the encrypted token of a repository, and an activity row for
/// `credential.set_by` (`repository_credential_set:<slug>` or
/// `repository_credential_replaced:<slug>`). A replacement clears the
/// result of the last check.
pub fn put(
    conn: &mut PgConnection,
    credential: NewCredential,
    slug: &str,
) -> Result<PutOutcome, diesel::result::Error> {
    use repository_credentials::dsl;
    conn.transaction(|conn| {
        let existed: bool = diesel::select(diesel::dsl::exists(
            dsl::repository_credentials.filter(dsl::repository_id.eq(credential.repository_id)),
        ))
        .get_result(conn)?;
        let actor = credential.set_by;
        let repository_id = credential.repository_id;
        diesel::insert_into(dsl::repository_credentials)
            .values(&credential)
            .on_conflict(dsl::repository_id)
            .do_update()
            .set((
                dsl::ciphertext.eq(&credential.ciphertext),
                dsl::nonce.eq(&credential.nonce),
                dsl::key_id.eq(&credential.key_id),
                dsl::set_by.eq(credential.set_by),
                dsl::set_at.eq(Utc::now()),
                dsl::last_checked_at.eq(None::<DateTime<Utc>>),
                dsl::last_check_ok.eq(None::<bool>),
                dsl::last_check_error.eq(None::<String>),
            ))
            .execute(conn)?;
        let outcome = if existed {
            PutOutcome::Replaced
        } else {
            PutOutcome::Set
        };
        let verb = match outcome {
            PutOutcome::Set => "set",
            PutOutcome::Replaced => "replaced",
        };
        log_activity(
            conn,
            actor,
            repository_id,
            format!("repository_credential_{verb}:{slug}"),
        )?;
        Ok(outcome)
    })
}

/// The encrypted token of a repository, if it has one.
pub fn load(
    conn: &mut PgConnection,
    repository_id: Uuid,
) -> Result<Option<StoredCredential>, diesel::result::Error> {
    use repository_credentials::dsl;
    dsl::repository_credentials
        .filter(dsl::repository_id.eq(repository_id))
        .select(StoredCredential::as_select())
        .first(conn)
        .optional()
}

/// The status of the credential of each repository in `ids` that has one.
/// A repository with no token has no entry.
pub fn statuses(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, CredentialStatus>, diesel::result::Error> {
    use repository_credentials::dsl;
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    type Row = (
        Uuid,
        Uuid,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<bool>,
        Option<String>,
    );
    let rows: Vec<Row> = dsl::repository_credentials
        .filter(dsl::repository_id.eq_any(ids))
        .select((
            dsl::repository_id,
            dsl::set_by,
            dsl::set_at,
            dsl::last_checked_at,
            dsl::last_check_ok,
            dsl::last_check_error,
        ))
        .load(conn)?;
    let mut actors: Vec<Uuid> = rows.iter().map(|r| r.1).collect();
    actors.sort_unstable();
    actors.dedup();
    let names: HashMap<Uuid, String> = {
        use crate::schema::users;
        users::table
            .filter(users::id.eq_any(&actors))
            .select((users::id, users::display_name, users::email))
            .load::<(Uuid, String, String)>(conn)?
            .into_iter()
            .map(|(id, display_name, email)| {
                let name = if display_name.trim().is_empty() {
                    email
                } else {
                    display_name
                };
                (id, name)
            })
            .collect()
    };
    Ok(rows
        .into_iter()
        .map(
            |(repository_id, set_by, set_at, last_checked_at, last_check_ok, last_check_error)| {
                (
                    repository_id,
                    CredentialStatus {
                        repository_id,
                        set_by,
                        set_by_name: names.get(&set_by).cloned(),
                        set_at,
                        last_checked_at,
                        last_check_ok,
                        last_check_error,
                    },
                )
            },
        )
        .collect())
}

/// Remove the token of a repository, and write an activity row
/// (`repository_credential_removed:<slug>`) for `actor`. False: the
/// repository had no token, and nothing is written.
pub fn delete(
    conn: &mut PgConnection,
    repository_id: Uuid,
    actor: Uuid,
    slug: &str,
) -> Result<bool, diesel::result::Error> {
    use repository_credentials::dsl;
    conn.transaction(|conn| {
        let removed = diesel::delete(
            dsl::repository_credentials.filter(dsl::repository_id.eq(repository_id)),
        )
        .execute(conn)?;
        if removed == 0 {
            return Ok(false);
        }
        log_activity(
            conn,
            actor,
            repository_id,
            format!("repository_credential_removed:{slug}"),
        )?;
        Ok(true)
    })
}

/// Record the result of an access check. `error` must have no token in it:
/// the caller removes it first. False: the repository has no token now.
pub fn record_check(
    conn: &mut PgConnection,
    repository_id: Uuid,
    ok: bool,
    error: Option<&str>,
) -> Result<bool, diesel::result::Error> {
    use repository_credentials::dsl;
    let updated =
        diesel::update(dsl::repository_credentials.filter(dsl::repository_id.eq(repository_id)))
            .set((
                dsl::last_checked_at.eq(Some(Utc::now())),
                dsl::last_check_ok.eq(Some(ok)),
                dsl::last_check_error.eq(error),
            ))
            .execute(conn)?;
    Ok(updated > 0)
}

fn log_activity(
    conn: &mut PgConnection,
    actor_id: Uuid,
    repository_id: Uuid,
    details: String,
) -> Result<(), diesel::result::Error> {
    diesel::insert_into(crate::schema::activity_log::table)
        .values(NewActivityLogEntry {
            actor_id,
            action: ActivityAction::Repository,
            entity_id: Some(repository_id),
            entity_type: Some("repository".to_string()),
            details,
        })
        .execute(conn)?;
    Ok(())
}
