//! Repositories (KAIROS-I-0010, decision KAIROS-A-0019): the shared data
//! layer for repository-scoped work. Boards (the lens + repo chips), the
//! item detail's repository picker, the team page's repositories panel and
//! the admin directory all read the same mirrors from [`api`] — one home,
//! so the wire shape is declared once (KAIROS-T-0114).
//!
//! No route page lives here yet; the admin directory is
//! `pages::admin::repositories` and re-exports from this module.
//!
//! [`documents`] shows the documents and the ADRs that impact a
//! repository, and makes a new document for it (COLLIERY-T-0269). The
//! admin directory and the team page show it for each repository.

pub(crate) mod api;
pub(crate) mod code_index;
pub(crate) mod documents;

use api::Repository;

/// The options of a repository picker, as `(value, label)`: every live
/// repository of the tenant (COLLIERY-T-0221). The value is the slug, which
/// is what the server takes.
///
/// The repository of a task is an optional link that says where the code
/// is. A task on the board of any team may link to any repository
/// (COLLIERY-A-0023), so the picker offers them all. The repositories of
/// the team of the board come first, because that team works in them most.
/// Each group is in slug order, so the list does not move between loads.
///
/// A repository of a different team names its owning team in the label.
/// The reader then sees that the link crosses teams before the write, and
/// no warning is necessary after it.
///
/// `board_team` is the id of the team of the board. `None` puts every
/// repository in the second group. The result has no "(none)" option, and
/// it is empty when the tenant has no live repository: the caller then
/// shows no picker. Pure, host-tested.
pub(crate) fn picker_options(
    repositories: &[Repository],
    board_team: Option<&str>,
) -> Vec<(String, String)> {
    let own = |repository: &Repository| board_team == Some(repository.team.id.as_str());
    let mut ordered: Vec<&Repository> = repositories.iter().collect();
    // `false` sorts before `true`: the repositories of the board's team lead.
    ordered.sort_by(|a, b| (!own(a), &a.slug).cmp(&(!own(b), &b.slug)));
    ordered
        .into_iter()
        .map(|repository| {
            let mut label = repository.slug.clone();
            if !repository.repo_full_name.is_empty() {
                label.push_str(" · ");
                label.push_str(&repository.repo_full_name);
            }
            if !own(repository) {
                label.push_str(" · owner: ");
                label.push_str(&repository.team.name);
            }
            (repository.slug.clone(), label)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::api::RepositoryTeam;
    use super::*;

    fn repository(slug: &str, team: (&str, &str)) -> Repository {
        Repository {
            id: format!("r-{slug}"),
            slug: slug.to_string(),
            forge: "github".to_string(),
            repo_full_name: format!("acme/{slug}"),
            repo_url: format!("https://github.com/acme/{slug}"),
            default_branch: "main".to_string(),
            description: String::new(),
            team: RepositoryTeam {
                id: team.0.to_string(),
                slug: team.1.to_lowercase(),
                name: team.1.to_string(),
            },
            delivery_board_id: None,
            open_tasks: 0,
            has_webhook: false,
            credential: Default::default(),
            code_index_build: "on".to_string(),
        }
    }

    const PLATFORM: (&str, &str) = ("t-platform", "Platform");
    const WEB: (&str, &str) = ("t-web", "Web");
    const DATA: (&str, &str) = ("t-data", "Data");

    fn option(slug: &str, label: &str) -> (String, String) {
        (slug.to_string(), label.to_string())
    }

    /// COLLIERY-T-0221: the picker offers every repository of the tenant.
    /// The repositories of the team of the board are first and keep the
    /// label they had. Each repository of a different team comes after
    /// them and shows the name of its owning team.
    #[test]
    fn the_team_of_the_board_is_first_and_other_teams_show_their_owner() {
        // The server sends the directory in slug order, across teams.
        let directory = vec![
            repository("analytics", DATA),
            repository("payments-api", PLATFORM),
            repository("platform-infra", PLATFORM),
            repository("portal-web", WEB),
        ];
        assert_eq!(
            picker_options(&directory, Some("t-platform")),
            vec![
                option("payments-api", "payments-api · acme/payments-api"),
                option("platform-infra", "platform-infra · acme/platform-infra"),
                option("analytics", "analytics · acme/analytics · owner: Data"),
                option("portal-web", "portal-web · acme/portal-web · owner: Web"),
            ]
        );
        // The same directory on the board of a different team.
        assert_eq!(
            picker_options(&directory, Some("t-web")),
            vec![
                option("portal-web", "portal-web · acme/portal-web"),
                option("analytics", "analytics · acme/analytics · owner: Data"),
                option(
                    "payments-api",
                    "payments-api · acme/payments-api · owner: Platform"
                ),
                option(
                    "platform-infra",
                    "platform-infra · acme/platform-infra · owner: Platform"
                ),
            ]
        );
    }

    /// The order does not follow the order of the input: each group is in
    /// slug order, so two loads of the same directory give the same list.
    #[test]
    fn the_order_is_stable_for_any_input_order() {
        let sorted = vec![
            repository("payments-api", PLATFORM),
            repository("platform-infra", PLATFORM),
            repository("analytics", DATA),
            repository("portal-web", WEB),
        ];
        let mut shuffled = sorted.clone();
        shuffled.reverse();
        shuffled.swap(0, 2);
        let expected = picker_options(&sorted, Some("t-platform"));
        assert_eq!(picker_options(&shuffled, Some("t-platform")), expected);
        let slugs: Vec<&str> = expected.iter().map(|(slug, _)| slug.as_str()).collect();
        assert_eq!(
            slugs,
            ["payments-api", "platform-infra", "analytics", "portal-web"]
        );
    }

    /// A tenant with no live repository gives no options, so the caller
    /// shows no picker.
    #[test]
    fn an_empty_tenant_has_no_options() {
        assert!(picker_options(&[], Some("t-platform")).is_empty());
        assert!(picker_options(&[], None).is_empty());
    }

    /// The team of the board owns no repository, and the tenant has some:
    /// the picker offers them all, each with its owner. Until
    /// COLLIERY-T-0221 such a board had no picker. A board that has no
    /// team gives the same result.
    #[test]
    fn a_team_that_owns_no_repository_is_offered_those_of_other_teams() {
        let directory = vec![repository("portal-web", WEB), repository("analytics", DATA)];
        let expected = vec![
            option("analytics", "analytics · acme/analytics · owner: Data"),
            option("portal-web", "portal-web · acme/portal-web · owner: Web"),
        ];
        assert_eq!(picker_options(&directory, Some("t-platform")), expected);
        assert_eq!(picker_options(&directory, None), expected);
    }

    /// A repository with no forge full name shows its slug alone, then
    /// the owner when a different team owns it.
    #[test]
    fn a_label_without_a_full_name_starts_with_the_slug() {
        let mut bare = repository("infra", PLATFORM);
        bare.repo_full_name = String::new();
        assert_eq!(
            picker_options(std::slice::from_ref(&bare), Some("t-platform")),
            vec![option("infra", "infra")]
        );
        assert_eq!(
            picker_options(std::slice::from_ref(&bare), Some("t-web")),
            vec![option("infra", "infra · owner: Platform")]
        );
    }
}
