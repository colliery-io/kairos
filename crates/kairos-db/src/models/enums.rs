//! Rust enums for the KAIROS-S-0004 TEXT-CHECK columns (KAIROS-A-0009:
//! "enums stored as TEXT with CHECK constraints map to Rust enums via small
//! `FromSql`/`ToSql` impls").
//!
//! Each enum round-trips through `diesel::sql_types::Text`; deserializing a
//! value outside the CHECK set fails loudly with [`UnknownEnumValue`] rather
//! than defaulting.

use std::io::Write;

use diesel::deserialize::{self, FromSql, FromSqlRow};
use diesel::expression::AsExpression;
use diesel::pg::{Pg, PgValue};
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::Text;

/// A TEXT value read from the database that is not a member of the enum's
/// CHECK set (schema drift, hand-edited rows, ...).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {enum_name} value {value:?}")]
pub struct UnknownEnumValue {
    /// The Rust enum that rejected the value.
    pub enum_name: &'static str,
    /// The offending database value.
    pub value: String,
}

/// Declare a TEXT-backed enum: variants, their database strings, `FromStr`
/// (rejecting unknown values), `Display`, and diesel `FromSql`/`ToSql`.
macro_rules! text_enum {
    (
        $(#[$meta:meta])*
        $name:ident {
            $($variant:ident => $value:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, AsExpression, FromSqlRow,
        )]
        #[diesel(sql_type = Text)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The TEXT value stored in the database.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $($name::$variant => $value),+
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = UnknownEnumValue;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($value => Ok($name::$variant),)+
                    other => Err(UnknownEnumValue {
                        enum_name: stringify!($name),
                        value: other.to_string(),
                    }),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ToSql<Text, Pg> for $name {
            fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
                out.write_all(self.as_str().as_bytes())?;
                Ok(IsNull::No)
            }
        }

        impl FromSql<Text, Pg> for $name {
            fn from_sql(bytes: PgValue<'_>) -> deserialize::Result<Self> {
                let s = <String as FromSql<Text, Pg>>::from_sql(bytes)?;
                Ok(s.parse::<$name>()?)
            }
        }
    };
}

text_enum! {
    /// `public.organization_members.role` (`'admin' | 'member'`).
    OrgRole {
        Admin => "admin",
        Member => "member",
    }
}

text_enum! {
    /// `metadata_definitions.field_type` / `public.system_metadata_definitions.field_type`.
    FieldType {
        String => "string",
        Enum => "enum",
        Date => "date",
    }
}

text_enum! {
    /// `teams.team_type` (Team Topologies types, KAIROS-A-0001).
    TeamType {
        StreamAligned => "stream_aligned",
        Platform => "platform",
        Enabling => "enabling",
        ComplicatedSubsystem => "complicated_subsystem",
    }
}

text_enum! {
    /// `boards.board_level` / `public.system_board_defaults.board_level`
    /// (KAIROS-A-0002 flight levels).
    BoardLevel {
        Strategy => "strategy",
        Initiative => "initiative",
        Delivery => "delivery",
        Adr => "adr",
    }
}

text_enum! {
    /// `tasks.task_type`.
    TaskType {
        Task => "task",
        Bug => "bug",
        TechDebt => "tech_debt",
        Support => "support",
    }
}

text_enum! {
    /// `tasks.work_class` (KAIROS-T-0077): the Planned/Support lane axis —
    /// was this work planned, or did it arrive as unplanned intake?
    /// Orthogonal to [`TaskType`] (a bug can sit in either lane).
    WorkClass {
        Planned => "planned",
        Support => "support",
    }
}

text_enum! {
    /// `team_pages.kind` (KAIROS-T-0082): a folder groups pages; a page
    /// carries markdown content.
    TeamPageKind {
        Folder => "folder",
        Page => "page",
    }
}

text_enum! {
    /// `forge_connections.forge` (KAIROS-T-0097): which git host a
    /// connection ingests from.
    Forge {
        Github => "github",
        Gitlab => "gitlab",
    }
}

text_enum! {
    /// `item_links.kind` (KAIROS-T-0097): what the link points at. A
    /// pull request and a merge request are the same thing under
    /// different forge vocabulary — one variant, normalized on ingest.
    LinkKind {
        Branch => "branch",
        PullRequest => "pull_request",
    }
}

text_enum! {
    /// `item_links.state` (KAIROS-T-0097): the forge-side state of a
    /// link. `Merged` is distinct from `Closed` because the forges are
    /// (GitHub reports merged as `closed` + `merged: true`) and because
    /// the team rollup treats them differently.
    LinkState {
        Open => "open",
        Merged => "merged",
        Closed => "closed",
        Draft => "draft",
    }
}

text_enum! {
    /// `documents.lifecycle` (KAIROS-T-0078): the editorial state of a
    /// document — a label with free transitions, NEVER board position
    /// (the two-vocabulary rule: ticket status is a board column;
    /// document lifecycle is this).
    DocumentLifecycle {
        Draft => "draft",
        Review => "review",
        Published => "published",
        Archived => "archived",
    }
}

text_enum! {
    /// `initiatives.complexity` (t-shirt sizing).
    Complexity {
        Xs => "xs",
        S => "s",
        M => "m",
        L => "l",
        Xl => "xl",
    }
}

text_enum! {
    /// `initiatives.bucket_type` (set iff `is_bucket`).
    BucketType {
        TechDebt => "tech_debt",
        Bug => "bug",
        AdHoc => "ad_hoc",
    }
}

text_enum! {
    /// `item_relationships.relationship` (KAIROS-A-0001 graph edge types).
    RelationshipType {
        Parent => "parent",
        Supports => "supports",
        Informs => "informs",
        Supersedes => "supersedes",
        Blocks => "blocks",
    }
}

text_enum! {
    /// `activity_log.action` (KAIROS-A-0004 audit actions). The column has
    /// no CHECK constraint (the set is documented in the S-0004 DDL comment),
    /// so this enum is the enforcement point. `board_config` extends the
    /// documented set for board configuration changes (column add/rename/
    /// remove/reorder, transition add/remove — KAIROS-T-0010; A-0004 itself
    /// already extends the set with `retention_sweep`).
    ActivityAction {
        Transition => "transition",
        Create => "create",
        Delete => "delete",
        RelationshipAdd => "relationship_add",
        RelationshipRemove => "relationship_remove",
        CapabilityGrant => "capability_grant",
        CapabilityRevoke => "capability_revoke",
        BoardConfig => "board_config",
        WorkClass => "work_class",
        Lifecycle => "lifecycle",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip every variant through its TEXT representation and reject
    /// values outside the CHECK set.
    macro_rules! assert_text_enum {
        ($name:ident, [$($value:literal),+ $(,)?]) => {
            // as_str -> parse round-trips every variant.
            for variant in $name::ALL {
                let parsed: $name = variant.as_str().parse().unwrap_or_else(|e| {
                    panic!("{} round-trip failed: {e}", stringify!($name))
                });
                assert_eq!(&parsed, variant);
                assert_eq!(variant.to_string(), variant.as_str());
            }
            // The database strings are exactly the S-0004 CHECK set, in order.
            let expected: &[&str] = &[$($value),+];
            let actual: Vec<&str> = $name::ALL.iter().map(|v| v.as_str()).collect();
            assert_eq!(actual, expected, "{} value set", stringify!($name));
            // Unknown values are rejected, not defaulted.
            for bogus in ["", "bogus", "ADMIN", "Tech_Debt", " task"] {
                let err = bogus.parse::<$name>().expect_err(
                    concat!(stringify!($name), " must reject unknown values"),
                );
                assert_eq!(err.enum_name, stringify!($name));
                assert_eq!(err.value, bogus);
            }
        };
    }

    #[test]
    fn org_role_round_trip_and_rejection() {
        assert_text_enum!(OrgRole, ["admin", "member"]);
    }

    #[test]
    fn field_type_round_trip_and_rejection() {
        assert_text_enum!(FieldType, ["string", "enum", "date"]);
    }

    #[test]
    fn team_type_round_trip_and_rejection() {
        assert_text_enum!(
            TeamType,
            [
                "stream_aligned",
                "platform",
                "enabling",
                "complicated_subsystem"
            ]
        );
    }

    #[test]
    fn board_level_round_trip_and_rejection() {
        assert_text_enum!(BoardLevel, ["strategy", "initiative", "delivery", "adr"]);
    }

    #[test]
    fn task_type_round_trip_and_rejection() {
        assert_text_enum!(TaskType, ["task", "bug", "tech_debt", "support"]);
    }

    #[test]
    fn work_class_round_trip_and_rejection() {
        assert_text_enum!(WorkClass, ["planned", "support"]);
    }

    #[test]
    fn team_page_kind_round_trip_and_rejection() {
        assert_text_enum!(TeamPageKind, ["folder", "page"]);
    }

    #[test]
    fn forge_round_trip_and_rejection() {
        assert_text_enum!(Forge, ["github", "gitlab"]);
    }

    #[test]
    fn link_kind_round_trip_and_rejection() {
        assert_text_enum!(LinkKind, ["branch", "pull_request"]);
    }

    #[test]
    fn link_state_round_trip_and_rejection() {
        assert_text_enum!(LinkState, ["open", "merged", "closed", "draft"]);
    }

    #[test]
    fn document_lifecycle_round_trip_and_rejection() {
        assert_text_enum!(
            DocumentLifecycle,
            ["draft", "review", "published", "archived"]
        );
    }

    #[test]
    fn complexity_round_trip_and_rejection() {
        assert_text_enum!(Complexity, ["xs", "s", "m", "l", "xl"]);
    }

    #[test]
    fn bucket_type_round_trip_and_rejection() {
        assert_text_enum!(BucketType, ["tech_debt", "bug", "ad_hoc"]);
    }

    #[test]
    fn relationship_type_round_trip_and_rejection() {
        assert_text_enum!(
            RelationshipType,
            ["parent", "supports", "informs", "supersedes", "blocks"]
        );
    }

    #[test]
    fn activity_action_round_trip_and_rejection() {
        assert_text_enum!(
            ActivityAction,
            [
                "transition",
                "create",
                "delete",
                "relationship_add",
                "relationship_remove",
                "capability_grant",
                "capability_revoke",
                "board_config",
                "work_class",
                "lifecycle",
            ]
        );
    }
}
