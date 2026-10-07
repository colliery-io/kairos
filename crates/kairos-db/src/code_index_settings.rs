//! The provider settings of the code index of a tenant (KAIROS-T-0339,
//! COLLIERY-I-0611): where the summaries are made (`embedded`,
//! `ollama-cloud`, `bedrock`) and where the vectors of the code index are
//! made (`embedded`, `remote`), with each secret sealed by the server
//! (`kairos-server::secrets`). One row at most; a tenant with no row uses
//! the embedded model for both.
//!
//! The connection must be pinned to a tenant schema. This module does not
//! check rights and does not open a secret: the server does both.

use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use uuid::Uuid;

use crate::schema::code_index_settings;

/// The providers of the summaries. The column has a CHECK with the same
/// values.
pub const SUMMARY_PROVIDERS: [&str; 3] = ["embedded", "ollama-cloud", "bedrock"];

/// The providers of the vectors of the code index.
pub const VECTOR_PROVIDERS: [&str; 2] = ["embedded", "remote"];

/// The range of `concurrency`.
pub const CONCURRENCY_RANGE: std::ops::RangeInclusive<i32> = 1..=32;

/// The default of `concurrency`.
pub const DEFAULT_CONCURRENCY: i32 = 4;

/// The row of a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Insertable, AsChangeset)]
#[diesel(table_name = code_index_settings)]
#[diesel(treat_none_as_null = true)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Settings {
    pub id: i32,
    pub summary_provider: String,
    pub summary_base_url: Option<String>,
    pub summary_model: Option<String>,
    pub summary_region: Option<String>,
    pub summary_ciphertext: Option<Vec<u8>>,
    pub summary_nonce: Option<Vec<u8>>,
    pub summary_key_id: Option<String>,
    pub summary_secret_set_by: Option<Uuid>,
    pub summary_secret_set_at: Option<DateTime<Utc>>,
    pub vector_provider: String,
    pub vector_base_url: Option<String>,
    pub vector_model: Option<String>,
    pub vector_ciphertext: Option<Vec<u8>>,
    pub vector_nonce: Option<Vec<u8>>,
    pub vector_key_id: Option<String>,
    pub vector_secret_set_by: Option<Uuid>,
    pub vector_secret_set_at: Option<DateTime<Utc>>,
    pub concurrency: i32,
    pub updated_by: Uuid,
    pub updated_at: DateTime<Utc>,
}

impl Settings {
    /// The settings of a tenant with no row: the embedded model for both.
    pub fn embedded(updated_by: Uuid) -> Self {
        Settings {
            id: 1,
            summary_provider: "embedded".to_string(),
            summary_base_url: None,
            summary_model: None,
            summary_region: None,
            summary_ciphertext: None,
            summary_nonce: None,
            summary_key_id: None,
            summary_secret_set_by: None,
            summary_secret_set_at: None,
            vector_provider: "embedded".to_string(),
            vector_base_url: None,
            vector_model: None,
            vector_ciphertext: None,
            vector_nonce: None,
            vector_key_id: None,
            vector_secret_set_by: None,
            vector_secret_set_at: None,
            concurrency: DEFAULT_CONCURRENCY,
            updated_by,
            updated_at: Utc::now(),
        }
    }

    /// The sealed secret of the summaries, if one is set.
    pub fn summary_secret(&self) -> Option<SealedSecret> {
        sealed(
            &self.summary_ciphertext,
            &self.summary_nonce,
            &self.summary_key_id,
        )
    }

    /// The sealed secret of the vectors, if one is set.
    pub fn vector_secret(&self) -> Option<SealedSecret> {
        sealed(
            &self.vector_ciphertext,
            &self.vector_nonce,
            &self.vector_key_id,
        )
    }
}

fn sealed(
    ciphertext: &Option<Vec<u8>>,
    nonce: &Option<Vec<u8>>,
    key_id: &Option<String>,
) -> Option<SealedSecret> {
    Some(SealedSecret {
        ciphertext: ciphertext.clone()?,
        nonce: nonce.clone()?,
        key_id: key_id.clone()?,
    })
}

/// A secret as the row keeps it: the server seals and opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedSecret {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub key_id: String,
}

/// What a write does to a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretChange {
    /// The stored secret stays.
    Keep,
    /// The stored secret goes.
    Remove,
    /// This sealed secret replaces the stored one.
    Set(SealedSecret),
}

/// The values of a write. The secrets are changes, so a write that does
/// not name a secret keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub summary_provider: String,
    pub summary_base_url: Option<String>,
    pub summary_model: Option<String>,
    pub summary_region: Option<String>,
    pub summary_secret: SecretChange,
    pub vector_provider: String,
    pub vector_base_url: Option<String>,
    pub vector_model: Option<String>,
    pub vector_secret: SecretChange,
    pub concurrency: i32,
    pub updated_by: Uuid,
}

/// The row of the tenant, if it has one.
pub fn load(conn: &mut PgConnection) -> QueryResult<Option<Settings>> {
    code_index_settings::table
        .find(1)
        .select(Settings::as_select())
        .first(conn)
        .optional()
}

/// The row of the tenant, or the embedded defaults. `updated_by` of the
/// defaults is the nil UUID.
pub fn load_or_default(conn: &mut PgConnection) -> QueryResult<Settings> {
    Ok(load(conn)?.unwrap_or_else(|| Settings::embedded(Uuid::nil())))
}

/// Write the settings of the tenant: the one row, made or replaced. A
/// secret that the update keeps stays with who set it and when.
pub fn save(conn: &mut PgConnection, update: Update) -> QueryResult<Settings> {
    conn.transaction(|conn| {
        let current = load(conn)?;
        let now = Utc::now();
        let (s_cipher, s_nonce, s_key, s_by, s_at) = apply(
            &update.summary_secret,
            current.as_ref().and_then(|c| {
                Some((
                    c.summary_ciphertext.clone()?,
                    c.summary_nonce.clone()?,
                    c.summary_key_id.clone()?,
                    c.summary_secret_set_by?,
                    c.summary_secret_set_at?,
                ))
            }),
            update.updated_by,
            now,
        );
        let (v_cipher, v_nonce, v_key, v_by, v_at) = apply(
            &update.vector_secret,
            current.as_ref().and_then(|c| {
                Some((
                    c.vector_ciphertext.clone()?,
                    c.vector_nonce.clone()?,
                    c.vector_key_id.clone()?,
                    c.vector_secret_set_by?,
                    c.vector_secret_set_at?,
                ))
            }),
            update.updated_by,
            now,
        );
        let row = Settings {
            id: 1,
            summary_provider: update.summary_provider,
            summary_base_url: update.summary_base_url,
            summary_model: update.summary_model,
            summary_region: update.summary_region,
            summary_ciphertext: s_cipher,
            summary_nonce: s_nonce,
            summary_key_id: s_key,
            summary_secret_set_by: s_by,
            summary_secret_set_at: s_at,
            vector_provider: update.vector_provider,
            vector_base_url: update.vector_base_url,
            vector_model: update.vector_model,
            vector_ciphertext: v_cipher,
            vector_nonce: v_nonce,
            vector_key_id: v_key,
            vector_secret_set_by: v_by,
            vector_secret_set_at: v_at,
            concurrency: update.concurrency,
            updated_by: update.updated_by,
            updated_at: now,
        };
        diesel::insert_into(code_index_settings::table)
            .values(&row)
            .on_conflict(code_index_settings::id)
            .do_update()
            .set(&row)
            .returning(Settings::as_select())
            .get_result(conn)
    })
}

type Stored = (Vec<u8>, Vec<u8>, String, Uuid, DateTime<Utc>);
type Columns = (
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<String>,
    Option<Uuid>,
    Option<DateTime<Utc>>,
);

/// The secret columns after a change.
fn apply(change: &SecretChange, stored: Option<Stored>, by: Uuid, now: DateTime<Utc>) -> Columns {
    match change {
        SecretChange::Keep => match stored {
            Some((c, n, k, b, a)) => (Some(c), Some(n), Some(k), Some(b), Some(a)),
            None => (None, None, None, None, None),
        },
        SecretChange::Remove => (None, None, None, None, None),
        SecretChange::Set(sealed) => (
            Some(sealed.ciphertext.clone()),
            Some(sealed.nonce.clone()),
            Some(sealed.key_id.clone()),
            Some(by),
            Some(now),
        ),
    }
}
