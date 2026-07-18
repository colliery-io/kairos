//! Run-world setup and end-of-run invariants (KAIROS-T-0046):
//!
//! - workforce enrollment on the `seed-demo` fixture tenant (JIT the
//!   `svc` service identity, org membership, `*` capability grants on the
//!   delivery boards for the non-admin actors);
//! - the shared soak fixtures (a soak initiative, contended collision
//!   tasks, the traverse root);
//! - the **bystander tenant**: provisioned via `/api/admin/tenants`, its
//!   FULL API-visible state snapshotted at start and byte-compared during
//!   and after the run — any change under a workload that never addresses
//!   it is a tenant-isolation breach (FATAL);
//! - the **history boundedness** check (A-0004, pragmatic form recorded
//!   in the task doc: the retention sweeper is not yet wired into
//!   `serve()` — M2 — so the invariant asserted here is that
//!   `item_history` grows ONLY with edits: rows per item == the item's
//!   version, and stays under a hard cap).

use std::collections::BTreeMap;

use kairos_client::types::Pagination;
use kairos_client::types::{CreateInitiativeRequest, CreateStrategyRequest, CreateTaskRequest};
use kairos_client::types_meta::ActivityQuery;
use kairos_client::types_org::{
    AddBoardMemberRequest, AddOrgMemberRequest, CreateTenantRequest, ReplaceCapabilitiesRequest,
};
use kairos_client::{EntityKind, Error, KairosClient};

use crate::stats::{Breach, Severity};

/// A delivery board with its column set and transition adjacency.
#[derive(Debug, Clone)]
pub struct BoardInfo {
    pub id: String,
    pub slug: String,
    /// `column_id -> allowed target column ids`.
    pub targets: BTreeMap<String, Vec<String>>,
}

/// Everything the workers share.
#[derive(Debug)]
pub struct World {
    pub delivery_boards: Vec<BoardInfo>,
    /// Contended tasks for the deliberate 409 mix.
    pub collision_codes: Vec<String>,
    /// Traverse root (the seeded strategy).
    pub strategy_code: String,
    /// The initiative this run's tasks notionally belong to.
    pub soak_initiative_code: String,
    /// Rotating full-text search terms (present in seed + soak content).
    pub search_terms: Vec<&'static str>,
}

impl World {
    pub fn board_by_id(&self, id: &str) -> Option<&BoardInfo> {
        self.delivery_boards.iter().find(|b| b.id == id)
    }
}

fn setup_err(stage: &str, e: impl std::fmt::Display) -> String {
    format!("setup ({stage}): {e}")
}

/// Enroll the workforce and create the shared fixtures. `clients` are the
/// tenant-scoped clients for (alice, bob, carol, svc); alice must be the
/// seeded org admin.
pub async fn setup_world(
    alice: &KairosClient,
    others: &[(&str, &KairosClient)],
) -> Result<World, String> {
    // alice: identity + admin sanity.
    let who = alice
        .whoami()
        .await
        .map_err(|e| setup_err("alice whoami", e))?;
    if who.organization.role != "admin" {
        return Err(format!(
            "setup: alice is {:?}, not the seeded org admin — wrong tenant or fixture?",
            who.organization.role
        ));
    }

    // JIT-provision the others (a 403 MEMBERSHIP_REQUIRED still upserts
    // the user row — the auth layer runs before the membership gate), then
    // make sure each is an org member. `svc` is the one the fixture does
    // not enroll.
    let mut member_ids: BTreeMap<String, String> = BTreeMap::new();
    for (name, client) in others {
        match client.whoami().await {
            Ok(who) => {
                member_ids.insert((*name).to_string(), who.user.id);
            }
            Err(Error::Forbidden { .. }) => {
                let email = format!("{name}@kairos.test");
                match alice
                    .add_org_member(&AddOrgMemberRequest {
                        email: email.clone(),
                        role: Some("member".to_string()),
                    })
                    .await
                {
                    Ok(member) => {
                        member_ids.insert((*name).to_string(), member.user_id);
                    }
                    Err(Error::Conflict { .. }) => {
                        // Already a member (a previous soak run): resolve id.
                        let members = alice
                            .list_org_members(Pagination {
                                limit: Some(200),
                                offset: None,
                            })
                            .await
                            .map_err(|e| setup_err("list members", e))?;
                        let found = members
                            .items
                            .iter()
                            .find(|m| m.email == email)
                            .ok_or_else(|| format!("setup: {email} not in member list"))?;
                        member_ids.insert((*name).to_string(), found.user_id.clone());
                    }
                    Err(e) => return Err(setup_err(&format!("enroll {name}"), e)),
                }
            }
            Err(e) => return Err(setup_err(&format!("{name} whoami"), e)),
        }
    }

    // Board discovery: every delivery board + the initiative/strategy
    // boards from the seed.
    let boards = alice
        .list_boards(Pagination {
            limit: Some(100),
            offset: None,
        })
        .await
        .map_err(|e| setup_err("list boards", e))?;
    let mut delivery_boards = Vec::new();
    let mut initiative_board = None;
    let mut strategy_board = None;
    for board in &boards.items {
        match board.board_level.as_str() {
            "delivery" => {
                let detail = alice
                    .get_board(&board.id)
                    .await
                    .map_err(|e| setup_err(&format!("board {}", board.slug), e))?;
                let mut targets: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for t in &detail.transitions {
                    targets
                        .entry(t.from_column_id.clone())
                        .or_default()
                        .push(t.to_column_id.clone());
                }
                delivery_boards.push(BoardInfo {
                    id: board.id.clone(),
                    slug: board.slug.clone(),
                    targets,
                });
            }
            "initiative" => initiative_board = Some(board.id.clone()),
            "strategy" => strategy_board = Some(board.id.clone()),
            _ => {}
        }
    }
    if delivery_boards.is_empty() {
        return Err("setup: no delivery boards found — is the demo tenant seeded?".to_string());
    }
    let initiative_board =
        initiative_board.ok_or("setup: no initiative board — is the demo tenant seeded?")?;

    // Grants: the non-admin workforce needs write capabilities on the
    // delivery boards (KAIROS-A-0006 whitelist; org admins bypass).
    for board in &delivery_boards {
        for (name, user_id) in &member_ids {
            match alice
                .add_board_member(
                    &board.id,
                    &AddBoardMemberRequest {
                        user_id: user_id.clone(),
                        capabilities: vec!["*".to_string()],
                    },
                )
                .await
            {
                Ok(_) => {}
                Err(Error::Conflict { .. }) => {
                    // Existing member from a previous run: make the set `*`.
                    alice
                        .replace_capabilities(
                            &board.id,
                            user_id,
                            &ReplaceCapabilitiesRequest {
                                capabilities: vec!["*".to_string()],
                            },
                        )
                        .await
                        .map_err(|e| {
                            setup_err(&format!("capabilities {} on {}", name, board.slug), e)
                        })?;
                }
                Err(e) => {
                    return Err(setup_err(&format!("grant {} on {}", name, board.slug), e));
                }
            }
        }
    }

    // Shared fixtures: the soak initiative and the contended collision
    // tasks (deliberate-409 targets).
    let stamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let initiative = alice
        .create_initiative(&CreateInitiativeRequest {
            board_id: initiative_board,
            column_id: None,
            title: format!("Soak workload {stamp}"),
            content: "Container for KAIROS-T-0046 soak-run workload items.".to_string(),
            complexity: Some("l".to_string()),
            bucket_type: None,
        })
        .await
        .map_err(|e| setup_err("soak initiative", e))?;

    let mut collision_codes = Vec::new();
    for n in 1..=2 {
        let task = alice
            .create_task(&CreateTaskRequest {
                board_id: delivery_boards[0].id.clone(),
                column_id: None,
                title: format!("Soak collision item {n} ({stamp})"),
                content: "Contended item for deliberate optimistic-concurrency 409s.".to_string(),
                task_type: Some("task".to_string()),
                team_id: None,
            })
            .await
            .map_err(|e| setup_err("collision task", e))?;
        collision_codes.push(task.short_code);
    }

    // Traverse root: the seeded strategy (or one we create on an empty
    // strategy board).
    let strategies = alice
        .list_strategies(Pagination {
            limit: Some(1),
            offset: None,
        })
        .await
        .map_err(|e| setup_err("list strategies", e))?;
    let strategy_code = match strategies.items.first() {
        Some(s) => s.short_code.clone(),
        None => {
            let board_id =
                strategy_board.ok_or("setup: no strategy board — is the demo tenant seeded?")?;
            alice
                .create_strategy(&CreateStrategyRequest {
                    board_id,
                    column_id: None,
                    title: "Soak traverse root".to_string(),
                    content: "Traverse root for the soak run.".to_string(),
                    hypothesis: None,
                })
                .await
                .map_err(|e| setup_err("traverse root", e))?
                .short_code
        }
    };

    Ok(World {
        delivery_boards,
        collision_codes,
        strategy_code,
        soak_initiative_code: initiative.short_code,
        search_terms: vec!["portal", "billing", "sign-up", "webhook", "soak", "form"],
    })
}

// ---------------------------------------------------------------------------
// Bystander tenant (tenant-isolation invariant)
// ---------------------------------------------------------------------------

/// The bystander tenant and its baseline snapshot.
pub struct Bystander {
    pub client: KairosClient,
    pub tenant: String,
    pub baseline: BTreeMap<String, String>,
}

/// Why the bystander check could not run (reported loudly either way).
#[derive(Debug)]
pub enum BystanderUnavailable {
    /// The caller is not in `KAIROS_DEPLOYMENT_ADMINS` on this deployment.
    NotDeploymentAdmin(String),
    /// Any other setup failure.
    Failed(String),
}

/// Provision (or adopt) the bystander tenant and take the baseline
/// snapshot. `admin` must be a client WITHOUT a tenant header whose user
/// is a deployment admin; `tenant_client` is the same identity scoped to
/// the bystander tenant.
pub async fn setup_bystander(
    admin: &KairosClient,
    tenant_client: KairosClient,
    slug: &str,
) -> Result<Bystander, BystanderUnavailable> {
    let created = match admin
        .create_tenant(&CreateTenantRequest {
            slug: slug.to_string(),
            name: "Soak Bystander".to_string(),
            initial_admin_external_id: None,
        })
        .await
    {
        Ok(_) => true,
        Err(Error::Conflict { .. }) => false, // exists from a previous run
        Err(Error::Forbidden { message, .. }) => {
            return Err(BystanderUnavailable::NotDeploymentAdmin(message));
        }
        Err(e) => return Err(BystanderUnavailable::Failed(e.to_string())),
    };

    if created {
        // Give the bystander real content so the checksum covers item
        // tables, not just provisioning defaults.
        let boards = tenant_client
            .list_boards(Pagination {
                limit: Some(50),
                offset: None,
            })
            .await
            .map_err(|e| BystanderUnavailable::Failed(format!("bystander boards: {e}")))?;
        let board_id = |level: &str| {
            boards
                .items
                .iter()
                .find(|b| b.board_level == level)
                .map(|b| b.id.clone())
                .ok_or_else(|| {
                    BystanderUnavailable::Failed(format!("bystander has no {level} board"))
                })
        };
        let strategy = tenant_client
            .create_strategy(&CreateStrategyRequest {
                board_id: board_id("strategy")?,
                column_id: None,
                title: "Bystander strategy".to_string(),
                content: "Must remain byte-identical through the soak run.".to_string(),
                hypothesis: None,
            })
            .await
            .map_err(|e| BystanderUnavailable::Failed(format!("bystander strategy: {e}")))?;
        let _ = strategy;
        tenant_client
            .create_initiative(&CreateInitiativeRequest {
                board_id: board_id("initiative")?,
                column_id: None,
                title: "Bystander initiative".to_string(),
                content: "Must remain byte-identical through the soak run.".to_string(),
                complexity: Some("s".to_string()),
                bucket_type: None,
            })
            .await
            .map_err(|e| BystanderUnavailable::Failed(format!("bystander initiative: {e}")))?;
    }

    let baseline = snapshot(&tenant_client)
        .await
        .map_err(BystanderUnavailable::Failed)?;
    Ok(Bystander {
        client: tenant_client,
        tenant: slug.to_string(),
        baseline,
    })
}

/// Serialize a list of JSON-serializable items into one canonical string
/// (each item serialized independently, then sorted — list order is not
/// part of the invariant, content is).
fn canonical<T: serde::Serialize>(items: &[T]) -> Result<String, String> {
    let mut lines: Vec<String> = items
        .iter()
        .map(|i| serde_json::to_string(i).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    lines.sort();
    Ok(lines.join("\n"))
}

/// The bystander's full API-visible state, section by section.
pub async fn snapshot(client: &KairosClient) -> Result<BTreeMap<String, String>, String> {
    let page = Pagination {
        limit: Some(200),
        offset: None,
    };
    let mut sections = BTreeMap::new();
    let err = |section: &str, e: Error| format!("bystander snapshot ({section}): {e}");

    let strategies = client
        .list_strategies(page)
        .await
        .map_err(|e| err("strategies", e))?;
    sections.insert(
        "strategies".to_string(),
        format!(
            "total={}\n{}",
            strategies.total,
            canonical(&strategies.items)?
        ),
    );
    let initiatives = client
        .list_initiatives(page)
        .await
        .map_err(|e| err("initiatives", e))?;
    sections.insert(
        "initiatives".to_string(),
        format!(
            "total={}\n{}",
            initiatives.total,
            canonical(&initiatives.items)?
        ),
    );
    let tasks = client.list_tasks(page).await.map_err(|e| err("tasks", e))?;
    sections.insert(
        "tasks".to_string(),
        format!("total={}\n{}", tasks.total, canonical(&tasks.items)?),
    );
    let documents = client
        .list_documents(page)
        .await
        .map_err(|e| err("documents", e))?;
    sections.insert(
        "documents".to_string(),
        format!(
            "total={}\n{}",
            documents.total,
            canonical(&documents.items)?
        ),
    );
    let adrs = client.list_adrs(page).await.map_err(|e| err("adrs", e))?;
    sections.insert(
        "adrs".to_string(),
        format!("total={}\n{}", adrs.total, canonical(&adrs.items)?),
    );
    let teams = client.list_teams(page).await.map_err(|e| err("teams", e))?;
    sections.insert(
        "teams".to_string(),
        format!("total={}\n{}", teams.total, canonical(&teams.items)?),
    );

    let boards = client
        .list_boards(page)
        .await
        .map_err(|e| err("boards", e))?;
    let mut board_details = Vec::new();
    for board in &boards.items {
        board_details.push(
            client
                .get_board(&board.id)
                .await
                .map_err(|e| err(&format!("board {}", board.slug), e))?,
        );
    }
    sections.insert("boards".to_string(), canonical(&board_details)?);

    let activity = client
        .activity(&ActivityQuery {
            entity_id: None,
            actor_id: None,
            action: None,
            since: None,
            limit: Some(200),
            offset: None,
        })
        .await
        .map_err(|e| err("activity", e))?;
    sections.insert(
        "activity".to_string(),
        format!("total={}\n{}", activity.total, canonical(&activity.items)?),
    );

    Ok(sections)
}

/// Compare a fresh snapshot against the baseline; returns the changed
/// section names (empty = isolation held).
pub fn changed_sections(
    baseline: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut changed = Vec::new();
    for (section, before) in baseline {
        match current.get(section) {
            Some(now) if now == before => {}
            Some(_) => changed.push(section.clone()),
            None => changed.push(format!("{section} (missing)")),
        }
    }
    for section in current.keys() {
        if !baseline.contains_key(section) {
            changed.push(format!("{section} (new)"));
        }
    }
    changed
}

// ---------------------------------------------------------------------------
// History boundedness (A-0004, pragmatic form — see module docs)
// ---------------------------------------------------------------------------

/// Per-item history summary for the report.
#[derive(Debug, serde::Serialize)]
pub struct HistoryBound {
    pub short_code: String,
    pub version: i32,
    pub history_rows: i64,
}

/// Assert `item_history` rows per item == item version (history grows
/// only with edits — creation writes v1, each edit exactly one row) and
/// stays under the configured cap.
pub async fn history_bound_check(
    client: &KairosClient,
    codes: &[String],
    max_rows_per_item: i64,
) -> (Vec<HistoryBound>, Vec<Breach>) {
    let mut bounds = Vec::new();
    let mut breaches = Vec::new();
    for code in codes {
        let task = match client.get_task(code).await {
            Ok(task) => task,
            Err(e) => {
                breaches.push(Breach::now(
                    Severity::Breach,
                    "history_bound",
                    format!("{code}: could not fetch item for the history check: {e}"),
                ));
                continue;
            }
        };
        let history = match client.history(EntityKind::Task, code, Some(1), None).await {
            Ok(envelope) => envelope,
            Err(e) => {
                breaches.push(Breach::now(
                    Severity::Breach,
                    "history_bound",
                    format!("{code}: could not fetch history: {e}"),
                ));
                continue;
            }
        };
        if history.total != i64::from(task.version) {
            breaches.push(Breach::now(
                Severity::Breach,
                "history_bound",
                format!(
                    "{code}: {} history rows for version {} — history must grow \
                     only with edits (A-0004)",
                    history.total, task.version
                ),
            ));
        }
        if history.total > max_rows_per_item {
            breaches.push(Breach::now(
                Severity::Breach,
                "history_bound",
                format!(
                    "{code}: {} history rows exceeds the configured cap {max_rows_per_item}",
                    history.total
                ),
            ));
        }
        bounds.push(HistoryBound {
            short_code: code.clone(),
            version: task.version,
            history_rows: history.total,
        });
    }
    (bounds, breaches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_sections_attributes_differences() {
        let mut baseline = BTreeMap::new();
        baseline.insert("tasks".to_string(), "a".to_string());
        baseline.insert("boards".to_string(), "b".to_string());

        // Identical: no changes.
        assert!(changed_sections(&baseline, &baseline.clone()).is_empty());

        // Modified + missing + new all attributed.
        let mut current = BTreeMap::new();
        current.insert("tasks".to_string(), "MUTATED".to_string());
        current.insert("activity".to_string(), "x".to_string());
        let changed = changed_sections(&baseline, &current);
        assert_eq!(
            changed,
            vec![
                "boards (missing)".to_string(),
                "tasks".to_string(),
                "activity (new)".to_string()
            ]
        );
    }
}
