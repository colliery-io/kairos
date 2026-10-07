//! KAIROS-T-0330 — the migration lock: a second holder waits until the
//! first one is done, and the lock is released also when the work fails.
//!
//! Against real Postgres from the compose stack. The lock is a session
//! advisory lock, so the test needs no scratch database: it uses the admin
//! database and touches no table.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DieselError;

use kairos_db::with_migration_lock;

const DEFAULT_DATABASE_URL: &str = "postgres://kairos:kairos@localhost:41432/kairos";

fn connect() -> PgConnection {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
    PgConnection::establish(&url)
        .expect("connecting to compose postgres (is the stack up? `angreal services up`)")
}

#[test]
fn a_second_holder_waits_and_a_failure_releases_the_lock() {
    let (held_tx, held_rx) = mpsc::channel();
    let first = std::thread::spawn(move || {
        let mut conn = connect();
        with_migration_lock(&mut conn, |_| {
            held_tx.send(()).expect("signal");
            std::thread::sleep(Duration::from_millis(800));
            Ok::<_, DieselError>(())
        })
        .expect("first holder");
    });
    held_rx.recv().expect("the first holder has the lock");

    let mut conn = connect();
    let started = Instant::now();
    with_migration_lock(&mut conn, |_| Ok::<_, DieselError>(())).expect("second holder");
    let waited = started.elapsed();
    first.join().expect("first thread");
    assert!(
        waited >= Duration::from_millis(500),
        "the second holder must wait for the first, waited {waited:?}"
    );

    // A failure inside releases the lock: a holder on a different
    // connection gets it at once.
    let failed: Result<(), DieselError> =
        with_migration_lock(&mut conn, |_| Err(DieselError::RollbackTransaction));
    assert!(failed.is_err());
    let mut other = connect();
    let started = Instant::now();
    with_migration_lock(&mut other, |_| Ok::<_, DieselError>(())).expect("after a failure");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the lock was released"
    );
}
