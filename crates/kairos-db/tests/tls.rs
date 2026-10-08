//! The async connections follow `sslmode` (src/tls.rs): the pool and the
//! LISTEN connector encrypt when the server offers TLS, and fall back to
//! plain text under `prefer` when it does not.
//!
//! The compose Postgres has no TLS, so it covers the fallback.
//! `KAIROS_TLS_DATABASE_URL` names a server that takes TLS only (as RDS with
//! `rds.force_ssl`); the encrypted cases run only when it is set.

use diesel::sql_query;
use diesel::sql_types::Bool;
use diesel_async::RunQueryDsl;

use kairos_db::TenantPool;
use kairos_db::tls::MakeRustlsConnect;

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

fn plain_database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

fn with_sslmode(url: &str, mode: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}sslmode={mode}")
}

#[derive(diesel::QueryableByName)]
struct Ssl {
    #[diesel(sql_type = Bool)]
    ssl: bool,
}

const SSL_OF_THIS_SESSION: &str = "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()";

async fn pool_ssl(url: &str) -> bool {
    let pool = TenantPool::new(url, 1).await.expect("building the pool");
    let mut conn = pool.public_conn().await.expect("checking a connection out");
    sql_query(SSL_OF_THIS_SESSION)
        .get_result::<Ssl>(&mut conn)
        .await
        .expect("reading pg_stat_ssl")
        .ssl
}

async fn listener_ssl(url: &str) -> bool {
    let (client, connection) = tokio_postgres::connect(url, MakeRustlsConnect::new())
        .await
        .expect("connecting with MakeRustlsConnect");
    tokio::spawn(connection);
    client
        .query_one(SSL_OF_THIS_SESSION, &[])
        .await
        .expect("reading pg_stat_ssl")
        .get(0)
}

#[tokio::test]
async fn prefer_falls_back_to_plain_text_without_server_tls() {
    let url = with_sslmode(&plain_database_url(), "prefer");
    assert!(!pool_ssl(&url).await);
    assert!(!listener_ssl(&url).await);
}

#[tokio::test]
async fn disable_stays_plain_text() {
    let url = with_sslmode(&plain_database_url(), "disable");
    assert!(!pool_ssl(&url).await);
}

#[tokio::test]
async fn require_and_prefer_encrypt_against_a_tls_only_server() {
    let Ok(base) = std::env::var("KAIROS_TLS_DATABASE_URL") else {
        eprintln!("KAIROS_TLS_DATABASE_URL is not set; skipping the TLS-only server cases");
        return;
    };
    for mode in ["require", "prefer"] {
        let url = with_sslmode(&base, mode);
        assert!(pool_ssl(&url).await, "pool, sslmode={mode}");
        assert!(listener_ssl(&url).await, "listener, sslmode={mode}");
    }
}
