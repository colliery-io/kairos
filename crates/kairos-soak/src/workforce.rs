//! The simulated workforce (KAIROS-A-0012 tier 5): N tokio workers, each
//! an authenticated identity (human or the `svc` service account),
//! executing the weighted operation mix at a sustained per-worker pace,
//! plus the standing `/ws/events` subscriber connections.
//!
//! Latency accounting: every recorded sample wraps ONE client call (the
//! composite ops record their reads under `read` and their writes under
//! the write class), except `mcp`, which is deliberately one sample for
//! the whole session (documented; it gets a session-sized budget).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use kairos_client::types::{CreateTaskRequest, Pagination, UpdateContentRequest};
use kairos_client::types_search::{SearchRequest, SearchTraverse, SearchTraverseFrom};
use kairos_client::{Error, KairosClient};
use tokio::sync::watch;

use crate::auth::{PasswordToken, SmallRng};
use crate::config::SoakConfig;
use crate::stats::{OpClass, Outcome, Recorder};
use crate::world::World;

/// Cap on each worker's created-item pool (edit/transition targets).
const POOL_CAP: usize = 200;

/// One worker's identity and wiring.
pub struct WorkerSpec {
    /// Report/attribution name (`alice`, `agent-2`, ...).
    pub name: String,
    /// Tenant-scoped client for this identity.
    pub client: KairosClient,
    /// Raw-token supply for the MCP transport.
    pub token: Arc<PasswordToken>,
    /// Index into `world.delivery_boards` this worker calls home.
    pub home_board: usize,
    /// RNG seed (deterministic per worker).
    pub seed: u64,
}

/// Record one client call under `class`. `conflict_expected` marks calls
/// where a 409 is a designed outcome (the contended collision items).
async fn record_call<T>(
    recorder: &Recorder,
    class: OpClass,
    conflict_expected: bool,
    fut: impl Future<Output = Result<T, Error>>,
) -> Option<T> {
    let start = Instant::now();
    match fut.await {
        Ok(value) => {
            recorder.record(class, start.elapsed(), Outcome::Ok);
            Some(value)
        }
        Err(Error::Conflict { .. }) if conflict_expected => {
            recorder.record(class, start.elapsed(), Outcome::ExpectedConflict);
            None
        }
        Err(e) => {
            recorder.record(class, start.elapsed(), Outcome::Error(e.to_string()));
            None
        }
    }
}

/// The worker loop: pick from the weighted mix, run, pace, until `stop`.
/// Returns the worker's created short codes (history-check sample).
pub async fn worker_loop(
    spec: WorkerSpec,
    world: Arc<World>,
    config: Arc<SoakConfig>,
    recorder: Arc<Recorder>,
    http: reqwest::Client,
    mut stop: watch::Receiver<bool>,
) -> Vec<String> {
    let mut rng = SmallRng::new(spec.seed);
    let mut pool: Vec<String> = Vec::new();
    let mut counter: u64 = 0;
    // Sustained pace: rate is fleet-wide; each worker owns an equal share.
    let workers = 3 + config.agent_workers;
    let base_gap = workers as f64 / config.rate;

    while !*stop.borrow() {
        counter += 1;
        let class = choose_op(&config, &mut rng, pool.is_empty());
        run_op(
            &spec, &world, &config, &recorder, &http, &mut rng, &mut pool, counter, class,
        )
        .await;

        // Jittered pacing (0.5x..1.5x the mean gap), interruptible.
        let gap = Duration::from_secs_f64(base_gap * (0.5 + rng.unit()));
        tokio::select! {
            _ = tokio::time::sleep(gap) => {}
            _ = stop.changed() => break,
        }
    }
    pool
}

/// Weighted op selection; ops that need the worker's own pool degrade to
/// `create` until the pool has items.
fn choose_op(config: &SoakConfig, rng: &mut SmallRng, pool_empty: bool) -> OpClass {
    let mix = &config.mix;
    let roll = rng.below(u64::from(mix.total())) as u32;
    let table = [
        (mix.create, OpClass::Create),
        (mix.edit, OpClass::Edit),
        (mix.conflict_edit, OpClass::ConflictEdit),
        (mix.transition, OpClass::Transition),
        (mix.read, OpClass::Read),
        (mix.search, OpClass::Search),
        (mix.traverse, OpClass::Traverse),
        (mix.mcp, OpClass::Mcp),
    ];
    let mut cumulative = 0;
    let mut chosen = OpClass::Read;
    for (weight, class) in table {
        cumulative += weight;
        if roll < cumulative {
            chosen = class;
            break;
        }
    }
    if pool_empty && matches!(chosen, OpClass::Edit | OpClass::Transition) {
        OpClass::Create
    } else {
        chosen
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_op(
    spec: &WorkerSpec,
    world: &World,
    config: &SoakConfig,
    recorder: &Recorder,
    http: &reqwest::Client,
    rng: &mut SmallRng,
    pool: &mut Vec<String>,
    counter: u64,
    class: OpClass,
) {
    let client = &spec.client;
    let home = &world.delivery_boards[spec.home_board];
    match class {
        OpClass::Create => {
            let board =
                &world.delivery_boards[rng.below(world.delivery_boards.len() as u64) as usize];
            let request = CreateTaskRequest {
                board_id: board.id.clone(),
                column_id: None,
                title: format!("Soak {} #{counter}", spec.name),
                content: format!(
                    "Soak workload item for {} (initiative {}).",
                    spec.name, world.soak_initiative_code
                ),
                task_type: Some("task".to_string()),
                team_id: None,
            };
            if let Some(task) = record_call(
                recorder,
                OpClass::Create,
                false,
                client.create_task(&request),
            )
            .await
            {
                if pool.len() >= POOL_CAP {
                    let slot = rng.below(POOL_CAP as u64) as usize;
                    pool[slot] = task.short_code;
                } else {
                    pool.push(task.short_code);
                }
            }
        }
        OpClass::Edit => {
            // Own-pool item: contention-free, so any 409 here is real.
            let code = pool[rng.below(pool.len() as u64) as usize].clone();
            let Some(task) =
                record_call(recorder, OpClass::Read, false, client.get_task(&code)).await
            else {
                return;
            };
            let request = UpdateContentRequest {
                title: None,
                content: format!(
                    "Soak edit #{counter} by {} at {}.",
                    spec.name,
                    chrono::Utc::now().to_rfc3339()
                ),
                version: task.version,
            };
            record_call(
                recorder,
                OpClass::Edit,
                false,
                client.update_task(&code, &request),
            )
            .await;
        }
        OpClass::ConflictEdit => {
            // Contended item: edit at the fetched version (a raced 409 is
            // expected here), then REPLAY the same version — guaranteed
            // stale, the designed 409.
            let code = world.collision_codes
                [rng.below(world.collision_codes.len() as u64) as usize]
                .clone();
            let Some(task) =
                record_call(recorder, OpClass::Read, false, client.get_task(&code)).await
            else {
                return;
            };
            let request = UpdateContentRequest {
                title: None,
                content: format!("Soak collision #{counter} by {}.", spec.name),
                version: task.version,
            };
            record_call(
                recorder,
                OpClass::Edit,
                true,
                client.update_task(&code, &request),
            )
            .await;
            let start = Instant::now();
            match client.update_task(&code, &request).await {
                Err(Error::Conflict { .. }) => {
                    recorder.record(
                        OpClass::ConflictEdit,
                        start.elapsed(),
                        Outcome::ExpectedConflict,
                    );
                }
                Ok(_) => recorder.record(
                    OpClass::ConflictEdit,
                    start.elapsed(),
                    Outcome::Error(format!(
                        "{code}: stale-version PATCH (v{}) unexpectedly succeeded — \
                         optimistic concurrency (A-0004) not enforced?",
                        request.version
                    )),
                ),
                Err(e) => recorder.record(
                    OpClass::ConflictEdit,
                    start.elapsed(),
                    Outcome::Error(e.to_string()),
                ),
            }
        }
        OpClass::Transition => {
            let code = pool[rng.below(pool.len() as u64) as usize].clone();
            let Some(task) =
                record_call(recorder, OpClass::Read, false, client.get_task(&code)).await
            else {
                return;
            };
            let Some(board) = world.board_by_id(&task.board_id) else {
                return;
            };
            let Some(targets) = board.targets.get(&task.column_id).filter(|t| !t.is_empty()) else {
                return; // terminal column; the GET above still counted
            };
            let target = &targets[rng.below(targets.len() as u64) as usize];
            record_call(
                recorder,
                OpClass::Transition,
                false,
                client.transition_task(&code, target),
            )
            .await;
        }
        OpClass::Read => match rng.below(3) {
            0 => {
                let code = if pool.is_empty() || rng.below(2) == 0 {
                    &world.collision_codes[rng.below(world.collision_codes.len() as u64) as usize]
                } else {
                    &pool[rng.below(pool.len() as u64) as usize]
                };
                record_call(recorder, OpClass::Read, false, client.get_task(code)).await;
            }
            1 => {
                record_call(recorder, OpClass::Read, false, client.board_items(&home.id)).await;
            }
            _ => {
                record_call(
                    recorder,
                    OpClass::Read,
                    false,
                    client.list_tasks(Pagination {
                        limit: Some(50),
                        offset: None,
                    }),
                )
                .await;
            }
        },
        OpClass::Search => {
            let term = world.search_terms[rng.below(world.search_terms.len() as u64) as usize];
            record_call(
                recorder,
                OpClass::Search,
                false,
                client.search(&SearchRequest {
                    q: Some(term.to_string()),
                    filter: None,
                    traverse: None,
                    sort: None,
                    limit: Some(25),
                    offset: None,
                }),
            )
            .await;
        }
        OpClass::Traverse => {
            record_call(
                recorder,
                OpClass::Traverse,
                false,
                client.search(&SearchRequest {
                    q: None,
                    filter: None,
                    traverse: Some(SearchTraverse {
                        from: SearchTraverseFrom {
                            short_code: Some(world.strategy_code.clone()),
                            id: None,
                        },
                        relationships: vec!["parent".to_string(), "blocks".to_string()],
                        direction: "outbound".to_string(),
                        depth: Some(3),
                    }),
                    sort: None,
                    limit: Some(50),
                    offset: None,
                }),
            )
            .await;
        }
        OpClass::Mcp => {
            let token = match spec.token.token().await {
                Ok(token) => token,
                Err(e) => {
                    recorder.record(
                        OpClass::Mcp,
                        Duration::ZERO,
                        Outcome::Error(format!("token: {e}")),
                    );
                    return;
                }
            };
            let start = Instant::now();
            let result = crate::mcp::run_session(
                http,
                client.base_url(),
                &config.tenant,
                &token,
                &home.slug,
                &world.collision_codes[0],
            )
            .await;
            match result {
                Ok(()) => recorder.record(OpClass::Mcp, start.elapsed(), Outcome::Ok),
                Err(e) => recorder.record(OpClass::Mcp, start.elapsed(), Outcome::Error(e)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// WS subscribers
// ---------------------------------------------------------------------------

/// Shared counters for the standing event subscribers.
#[derive(Debug, Default)]
pub struct WsCounters {
    pub connects: AtomicU64,
    pub events: AtomicU64,
    pub reconnects: AtomicU64,
}

/// A standing `/ws/events` subscriber: counts delivered thin events,
/// reconnecting (and counting the reconnect) on any stream failure.
pub async fn ws_subscriber(
    client: KairosClient,
    counters: Arc<WsCounters>,
    mut stop: watch::Receiver<bool>,
) {
    while !*stop.borrow() {
        match client.connect_events().await {
            Ok(mut stream) => {
                counters.connects.fetch_add(1, Ordering::Relaxed);
                loop {
                    tokio::select! {
                        _ = stop.changed() => return,
                        event = tokio::time::timeout(
                            Duration::from_secs(30),
                            stream.next_event(),
                        ) => match event {
                            Ok(Ok(_)) => {
                                counters.events.fetch_add(1, Ordering::Relaxed);
                            }
                            Ok(Err(_)) => {
                                counters.reconnects.fetch_add(1, Ordering::Relaxed);
                                break;
                            }
                            Err(_elapsed) => {} // idle is fine; keep listening
                        },
                    }
                }
            }
            Err(_) => {
                counters.reconnects.fetch_add(1, Ordering::Relaxed);
                tokio::select! {
                    _ = stop.changed() => return,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OpMix;

    #[test]
    fn op_choice_follows_weights_and_degrades_without_a_pool() {
        let config = SoakConfig {
            mix: OpMix {
                create: 0,
                edit: 1,
                conflict_edit: 0,
                transition: 0,
                read: 0,
                search: 0,
                traverse: 0,
                mcp: 0,
            },
            ..SoakConfig::default()
        };
        let mut rng = SmallRng::new(7);
        // Only `edit` has weight; with an empty pool it degrades to create.
        assert_eq!(choose_op(&config, &mut rng, true), OpClass::Create);
        assert_eq!(choose_op(&config, &mut rng, false), OpClass::Edit);

        // The default mix only ever yields classes with nonzero weight,
        // and every class appears over enough draws.
        let config = SoakConfig::default();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..10_000 {
            seen.insert(choose_op(&config, &mut rng, false).name());
        }
        assert_eq!(seen.len(), OpClass::ALL.len(), "{seen:?}");
    }
}
