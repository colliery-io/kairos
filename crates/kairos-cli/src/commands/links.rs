//! `kairos link` and `kairos unlink` (KAIROS-T-0508, KAIROS-T-0591): make
//! and remove a relationship of an item.
//!
//! `kairos link <SOURCE> <RELATIONSHIP> <TARGET>`. The source and the target
//! are those of the edge, as in the MCP tool `link_items`: `kairos link
//! ACME-I-0001 parent ACME-T-0002` makes ACME-I-0001 the parent of
//! ACME-T-0002. The relationships:
//!
//! - `parent`, `blocks`, `informs`, `supports`, `supersedes`, `amends`: an edge
//!   between two items (`POST /api/relationships`). The server applies the
//!   matrix of the edge types, the cycle check and the link rule.
//! - `impacts`: from a document or an ADR to a repository (slug or UUID).
//! - `external`: from an item to a URI, with an optional `--label`. The
//!   server applies the rule of the URI (the schemes `http`, `https` and
//!   `mailto`).
//!
//! `unlink` finds the edge in the relationships of the source and removes
//! it by its id.

use clap::{Args, ValueEnum};
use kairos_client::EntityKind;
use kairos_client::types_meta::{
    CreateRelationshipRequest, ExternalRef, ItemRelationshipsResponse, RelatedItem,
};

use crate::commands::repos::family_of;
use crate::context::{Common, client, print_json};
use crate::error::CliError;

/// The relationships of `kairos link` and `kairos unlink`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LinkRelationship {
    /// The source is the parent of the target: a strategy of an
    /// initiative, or an initiative of a task
    Parent,
    /// The target waits for the source (strategies, initiatives, tasks)
    Blocks,
    /// The source informs the target: a document or an ADR informs a
    /// workflow item, or a task informs a task
    Informs,
    /// The target (a document or an ADR) supports the source (a workflow
    /// item)
    Supports,
    /// The source ADR replaces the target ADR
    Supersedes,
    /// The source ADR changes a part of the target ADR (KAIROS-A-0069)
    Amends,
    /// The source (a document or an ADR) is about the target repository
    /// (slug or UUID). It gives no right
    Impacts,
    /// A URI with a label: an RFC, a design page, an issue that Kairos
    /// does not hold
    External,
}

impl LinkRelationship {
    /// The name of the relationship on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parent => "parent",
            Self::Blocks => "blocks",
            Self::Informs => "informs",
            Self::Supports => "supports",
            Self::Supersedes => "supersedes",
            Self::Amends => "amends",
            Self::Impacts => "impacts",
            Self::External => "external",
        }
    }
}

/// Make a relationship of an item
#[derive(Debug, Args)]
#[command(long_about = "Make a relationship of an item.\n\n\
                  SOURCE and TARGET are the two ends of the edge, as in the MCP tool \
                  link_items:\n\
                  - parent: SOURCE is the parent of TARGET.\n\
                  - blocks: TARGET waits for SOURCE.\n\
                  - informs: SOURCE informs TARGET.\n\
                  - supports: TARGET (a document or an ADR) supports SOURCE.\n\
                  - supersedes: SOURCE (an ADR) replaces TARGET (an ADR).\n\
                  - amends: SOURCE (an ADR) changes a part of TARGET (an ADR).\n\
                  - impacts: SOURCE (a document or an ADR) is about the repository TARGET \
                  (slug or UUID).\n\
                  - external: TARGET is a URI (http, https or mailto), with an optional \
                  --label.\n\n\
                  The server checks the types of the two items, and refuses a cycle of \
                  parent, blocks or amends. You must be able to edit one of the two items. An item \
                  has one parent: `set-parent` replaces it.")]
pub struct LinkArgs {
    /// Short code of the source item (any type)
    #[arg(value_name = "SOURCE")]
    pub short_code: String,
    /// The relationship
    #[arg(value_enum)]
    pub relationship: LinkRelationship,
    /// The short code of the target item; for `impacts`, a repository; for
    /// `external`, a URI
    #[arg(value_name = "TARGET")]
    pub target: String,
    /// The label of the reference, at most 200 characters. Only with
    /// `external`
    #[arg(long)]
    pub label: Option<String>,
    #[command(flatten)]
    pub common: Common,
}

/// Remove a relationship of an item
#[derive(Debug, Args)]
#[command(long_about = "Remove a relationship of an item.\n\n\
                  SOURCE, RELATIONSHIP and TARGET are those of `kairos link`. The rule is \
                  that of `kairos link`. To remove the supports edge of a document, you \
                  must be able to edit the document.")]
pub struct UnlinkArgs {
    /// Short code of the source item
    #[arg(value_name = "SOURCE")]
    pub short_code: String,
    /// The relationship
    #[arg(value_enum)]
    pub relationship: LinkRelationship,
    /// The short code of the target item; for `impacts`, a repository; for
    /// `external`, the URI of the reference
    #[arg(value_name = "TARGET")]
    pub target: String,
    #[command(flatten)]
    pub common: Common,
}

/// The text of one reference: `label <uri>` or `<uri>`.
pub(crate) fn reference_text(reference: &ExternalRef) -> String {
    match reference.label.as_deref().filter(|label| !label.is_empty()) {
        Some(label) => format!("{label} <{}>", reference.uri),
        None => format!("<{}>", reference.uri),
    }
}

/// The URI as the server stores it, for the comparison of `unlink`:
/// trimmed, with the scheme in lower case. The rest of the rule is the
/// server's; a URI that does not pass it is in no list.
pub(crate) fn stored_form(uri: &str) -> String {
    let uri = uri.trim();
    match uri.split_once(':') {
        Some((scheme, rest)) => format!("{}:{rest}", scheme.to_ascii_lowercase()),
        None => uri.to_string(),
    }
}

/// The meaning of an edge between two items, as a sentence.
fn edge_meaning(relationship: LinkRelationship, source: &str, target: &str) -> String {
    match relationship {
        LinkRelationship::Parent => format!("{source} is the parent of {target}."),
        LinkRelationship::Blocks => format!("{target} waits for {source}."),
        LinkRelationship::Informs => format!("{source} informs {target}."),
        LinkRelationship::Supports => format!("{target} supports {source}."),
        LinkRelationship::Supersedes => format!("{source} replaces {target}."),
        LinkRelationship::Amends => format!("{source} changes a part of {target}."),
        LinkRelationship::Impacts => format!("{source} is about the repository {target}."),
        LinkRelationship::External => format!("{source} links to {target}."),
    }
}

/// The line of a new edge between two items: the edge and its meaning.
fn made_line(relationship: LinkRelationship, source: &str, target: &str) -> String {
    format!(
        "Kairos made the link: {source} -[{}]-> {target}. {}",
        relationship.as_str(),
        edge_meaning(relationship, source, target)
    )
}

/// The line of a removed edge.
fn removed_line(relationship: LinkRelationship, source: &str, target: &str) -> String {
    format!(
        "Kairos removed the link: {source} -[{}]-> {target}.",
        relationship.as_str()
    )
}

/// The neighbour of `source` at the target end of an outgoing edge, by
/// the short code of the target (not case-sensitive).
fn find_outgoing<'a>(
    relationships: &'a ItemRelationshipsResponse,
    relationship: &str,
    target: &str,
) -> Option<&'a RelatedItem> {
    relationships
        .outgoing
        .iter()
        .filter(|group| group.relationship == relationship)
        .flat_map(|group| group.items.iter())
        .find(|item| item.short_code.eq_ignore_ascii_case(target))
}

/// The refusal of `--label` with a relationship that is not `external`.
fn check_label(relationship: LinkRelationship, label: Option<&str>) -> Result<(), CliError> {
    match (relationship, label) {
        (LinkRelationship::External, _) | (_, None) => Ok(()),
        (other, Some(_)) => Err(CliError::Failure(format!(
            "The option --label applies to the relationship external only, not to {}.",
            other.as_str()
        ))),
    }
}

impl LinkArgs {
    pub async fn run(self) -> Result<(), CliError> {
        check_label(self.relationship, self.label.as_deref())?;
        let kind = family_of(&self.short_code)?;
        let client = client(&self.common)?;
        match self.relationship {
            LinkRelationship::External => {
                let made = client
                    .add_external_ref(kind, &self.short_code, &self.target, self.label.as_deref())
                    .await?;
                if self.common.json {
                    return print_json(&made);
                }
                println!(
                    "Kairos made the link: {} -[external]-> {}.",
                    self.short_code,
                    reference_text(&made)
                );
            }
            LinkRelationship::Impacts => {
                let impact = client
                    .add_impact(kind, &self.short_code, &self.target)
                    .await?;
                if self.common.json {
                    return print_json(&impact);
                }
                println!(
                    "{}",
                    made_line(self.relationship, &self.short_code, &impact.repository.slug)
                );
            }
            relationship => {
                family_of(&self.target)?;
                let made = client
                    .create_relationship(&CreateRelationshipRequest {
                        source_short_code: self.short_code.clone(),
                        target_short_code: self.target.clone(),
                        relationship: relationship.as_str().to_string(),
                    })
                    .await?;
                if self.common.json {
                    return print_json(&made);
                }
                println!(
                    "{}",
                    made_line(
                        relationship,
                        &made.source_short_code,
                        &made.target_short_code
                    )
                );
            }
        }
        Ok(())
    }
}

impl UnlinkArgs {
    pub async fn run(self) -> Result<(), CliError> {
        let kind = family_of(&self.short_code)?;
        let client = client(&self.common)?;
        match self.relationship {
            LinkRelationship::External => self.unlink_external(&client, kind).await,
            LinkRelationship::Impacts => {
                let removed = client
                    .remove_impact(kind, &self.short_code, &self.target)
                    .await?;
                if self.common.json {
                    return print_json(&removed);
                }
                println!(
                    "{}",
                    removed_line(self.relationship, &self.short_code, &removed.repository)
                );
                Ok(())
            }
            relationship => {
                family_of(&self.target)?;
                let listed = client.relationships(kind, &self.short_code).await?;
                let Some(found) = find_outgoing(&listed, relationship.as_str(), &self.target)
                else {
                    return Err(CliError::Failure(format!(
                        "{} has no link {} -[{}]-> {}. `kairos {kind} show {}` shows its \
                         relationships.",
                        self.short_code,
                        self.short_code,
                        relationship.as_str(),
                        self.target,
                        self.short_code
                    )));
                };
                let removed = client.delete_relationship(&found.relationship_id).await?;
                if self.common.json {
                    return print_json(&removed);
                }
                println!(
                    "{}",
                    removed_line(relationship, &self.short_code, &found.short_code)
                );
                Ok(())
            }
        }
    }

    async fn unlink_external(
        &self,
        client: &kairos_client::KairosClient,
        kind: EntityKind,
    ) -> Result<(), CliError> {
        let wanted = stored_form(&self.target);
        let listed = client.item_external_refs(kind, &self.short_code).await?;
        let Some(found) = listed
            .external
            .into_iter()
            .find(|reference| reference.uri == wanted)
        else {
            return Err(CliError::Failure(format!(
                "{} has no external reference {wanted}. `kairos {} show {}` shows its references.",
                self.short_code, kind, self.short_code
            )));
        };
        let removed = client
            .remove_external_ref(kind, &self.short_code, &found.id)
            .await?;
        if self.common.json {
            return print_json(&removed);
        }
        println!(
            "Kairos removed the link: {} -[external]-> {}.",
            self.short_code,
            reference_text(&found)
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EXIT_FAILURE;
    use kairos_client::types_meta::RelationshipGroup;

    fn reference(uri: &str, label: Option<&str>) -> ExternalRef {
        ExternalRef {
            id: "r-1".into(),
            relationship: "external".into(),
            uri: uri.into(),
            label: label.map(str::to_string),
            created_by: "u-1".into(),
            created_at: "2026-10-15T00:00:00Z".into(),
        }
    }

    fn neighbour(edge: &str, code: &str) -> RelatedItem {
        RelatedItem {
            relationship_id: edge.into(),
            id: format!("id-{code}"),
            short_code: code.into(),
            entity_type: "task".into(),
            title: format!("title of {code}"),
            archived_at: None,
            done: false,
            cancelled: false,
        }
    }

    #[test]
    fn the_text_of_a_reference_is_the_label_and_the_uri() {
        assert_eq!(
            reference_text(&reference("https://example.com/rfc", Some("RFC 42"))),
            "RFC 42 <https://example.com/rfc>"
        );
        assert_eq!(
            reference_text(&reference("https://example.com/rfc", None)),
            "<https://example.com/rfc>"
        );
        assert_eq!(
            reference_text(&reference("https://example.com/rfc", Some(""))),
            "<https://example.com/rfc>"
        );
    }

    #[test]
    fn the_stored_form_lowers_the_scheme_only() {
        assert_eq!(
            stored_form("  HTTPS://Example.com/A "),
            "https://Example.com/A"
        );
        assert_eq!(stored_form("no-scheme"), "no-scheme");
    }

    /// KAIROS-T-0591: each relationship parses, and an unknown one is a
    /// usage error that names the values.
    #[test]
    fn link_and_unlink_take_each_relationship() {
        use clap::Parser;
        #[derive(Parser, Debug)]
        enum Cli {
            Link(LinkArgs),
            Unlink(UnlinkArgs),
        }
        for (word, relationship) in [
            ("parent", LinkRelationship::Parent),
            ("blocks", LinkRelationship::Blocks),
            ("informs", LinkRelationship::Informs),
            ("supports", LinkRelationship::Supports),
            ("supersedes", LinkRelationship::Supersedes),
            ("amends", LinkRelationship::Amends),
            ("impacts", LinkRelationship::Impacts),
            ("external", LinkRelationship::External),
        ] {
            assert_eq!(relationship.as_str(), word);
            match Cli::try_parse_from(["kairos", "link", "ACME-I-0001", word, "ACME-T-0002"])
                .expect("link parses")
            {
                Cli::Link(args) => assert_eq!(args.relationship, relationship),
                other => panic!("not link: {other:?}"),
            }
            match Cli::try_parse_from(["kairos", "unlink", "ACME-I-0001", word, "ACME-T-0002"])
                .expect("unlink parses")
            {
                Cli::Unlink(args) => assert_eq!(args.relationship, relationship),
                other => panic!("not unlink: {other:?}"),
            }
        }
        let refused =
            Cli::try_parse_from(["kairos", "link", "ACME-T-0001", "child", "ACME-T-0002"])
                .expect_err("an unknown relationship");
        assert_eq!(refused.kind(), clap::error::ErrorKind::InvalidValue);
        assert!(refused.to_string().contains("child"), "{refused}");
        assert!(refused.to_string().contains("supersedes"), "{refused}");
    }

    /// KAIROS-T-0591: `--label` is for `external` only, and the refusal
    /// names the relationship.
    #[test]
    fn a_label_is_for_external_only() {
        assert!(check_label(LinkRelationship::External, Some("RFC")).is_ok());
        assert!(check_label(LinkRelationship::Blocks, None).is_ok());
        let refused = check_label(LinkRelationship::Blocks, Some("RFC")).expect_err("refused");
        assert_eq!(refused.exit_code(), EXIT_FAILURE);
        assert!(refused.to_string().contains("--label"), "{refused}");
        assert!(refused.to_string().contains("not to blocks"), "{refused}");
    }

    /// KAIROS-T-0591: the line of `link` gives the edge and its meaning in
    /// the direction of the edge.
    #[test]
    fn the_line_of_a_link_says_what_the_edge_means() {
        assert_eq!(
            made_line(LinkRelationship::Parent, "ACME-I-0001", "ACME-T-0002"),
            "Kairos made the link: ACME-I-0001 -[parent]-> ACME-T-0002. ACME-I-0001 is the \
             parent of ACME-T-0002."
        );
        assert!(
            made_line(LinkRelationship::Blocks, "ACME-T-0001", "ACME-T-0002")
                .ends_with("ACME-T-0002 waits for ACME-T-0001.")
        );
        assert!(
            made_line(LinkRelationship::Supports, "ACME-I-0001", "ACME-D-0002")
                .ends_with("ACME-D-0002 supports ACME-I-0001.")
        );
        assert!(
            made_line(LinkRelationship::Informs, "ACME-D-0001", "ACME-T-0002")
                .ends_with("ACME-D-0001 informs ACME-T-0002.")
        );
        assert!(
            made_line(LinkRelationship::Supersedes, "ACME-A-0002", "ACME-A-0001")
                .ends_with("ACME-A-0002 replaces ACME-A-0001.")
        );
        assert!(
            made_line(LinkRelationship::Amends, "ACME-A-0003", "ACME-A-0001")
                .ends_with("ACME-A-0003 changes a part of ACME-A-0001.")
        );
        assert!(
            made_line(LinkRelationship::Impacts, "ACME-D-0001", "web")
                .ends_with("ACME-D-0001 is about the repository web.")
        );
        assert_eq!(
            removed_line(LinkRelationship::Blocks, "ACME-T-0001", "ACME-T-0002"),
            "Kairos removed the link: ACME-T-0001 -[blocks]-> ACME-T-0002."
        );
    }

    /// KAIROS-T-0591: `unlink` finds the edge among the outgoing edges of
    /// the source, of that relationship, by the code of the target.
    #[test]
    fn unlink_finds_the_outgoing_edge_of_the_relationship() {
        let listed = ItemRelationshipsResponse {
            short_code: "ACME-T-0001".into(),
            outgoing: vec![
                RelationshipGroup {
                    relationship: "informs".into(),
                    items: vec![neighbour("e-informs", "ACME-T-0002")],
                },
                RelationshipGroup {
                    relationship: "blocks".into(),
                    items: vec![
                        neighbour("e-blocks-3", "ACME-T-0003"),
                        neighbour("e-blocks-2", "ACME-T-0002"),
                    ],
                },
            ],
            incoming: vec![RelationshipGroup {
                relationship: "blocks".into(),
                items: vec![neighbour("e-incoming", "ACME-T-0004")],
            }],
        };
        let found = find_outgoing(&listed, "blocks", "acme-t-0002").expect("found");
        assert_eq!(found.relationship_id, "e-blocks-2");
        assert!(find_outgoing(&listed, "parent", "ACME-T-0002").is_none());
        // An incoming edge is not an edge of this source.
        assert!(find_outgoing(&listed, "blocks", "ACME-T-0004").is_none());
    }
}
