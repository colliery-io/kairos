//! Pure ABAC capability semantics (KAIROS-A-0006, KAIROS-T-0011): the
//! capability vocabulary, glob matching, and the authorization decision over
//! already-loaded grants.
//!
//! Per KAIROS-A-0009 everything here is a function over in-memory data — no
//! diesel, no async, no I/O. `kairos-db::abac` runs the single-query SQL
//! check; [`capability_matches`] is the pure mirror of that SQL's semantics
//! so both layers can never disagree.
//!
//! # Glob semantics (the A-0006 LIKE translation, exactly)
//!
//! A-0006 defines the access check as SQL:
//!
//! ```sql
//! $required LIKE replace(capability, '*', '%') OR capability = '*'
//! ```
//!
//! with one hardening refinement carried by both layers: LIKE's own
//! metacharacters (`%`, `_`, and the escape char `\`) occurring in a STORED
//! capability are escaped before the `*`→`%` translation, so they match
//! themselves literally and can never smuggle a wildcard into the check. The
//! resulting semantics, mirrored by [`capability_matches`]:
//!
//! - every `*` in the granted string matches any sequence of characters
//!   (including the empty sequence),
//! - every other character (including `%`, `_`, `\`) matches only itself,
//! - a bare `*` grant matches every required capability.
//!
//! The only SANCTIONED glob position in the A-0006 vocabulary is a trailing
//! `*` (`*`, `manage_*`, `configure_*`, `transition_*`). A `*` embedded
//! mid-string still wildcards — that is what the SQL translation does and
//! this function mirrors SQL truth rather than inventing a second dialect —
//! but such grants are outside the vocabulary and should be rejected at
//! grant time by API-layer validation, not relied upon.

// ---------------------------------------------------------------------------
// Capability vocabulary (A-0006)
// ---------------------------------------------------------------------------

/// Create, update, delete strategies.
pub const MANAGE_STRATEGIES: &str = "manage_strategies";
/// Create, update, delete initiatives.
pub const MANAGE_INITIATIVES: &str = "manage_initiatives";
/// Create, update, delete tasks.
pub const MANAGE_TASKS: &str = "manage_tasks";
/// Create, update, delete documents.
pub const MANAGE_DOCUMENTS: &str = "manage_documents";
/// Create, update, delete ADRs.
pub const MANAGE_ADRS: &str = "manage_adrs";
/// Move items between board columns.
pub const TRANSITION_ITEMS: &str = "transition_items";
/// Modify board columns, transitions, settings.
pub const CONFIGURE_BOARDS: &str = "configure_boards";
// `configure_templates` and `configure_metadata` were in this vocabulary and
// are deliberately NOT (KAIROS-T-0182, amending KAIROS-A-0006). A grant is
// keyed (board_id, user_id, capability), and `templates` and
// `metadata_definitions` have no board_id — they are tenant-wide, scoped by
// entity_type at most. So "configure_metadata on board X" could only ever have
// authorised edits affecting every board, which is org-admin authority wearing
// a board-scoped costume. Both stayed unchecked by any handler for their whole
// life; the admin UI rendered them beside two that worked. Making templates and
// metadata board-scoped is a schema change, not a bug fix — see
// KAIROS-T-0182 if that is ever wanted.
/// Add/remove users from the board, grant/revoke capabilities.
///
/// Named `administer_members` and NOT `manage_members` (KAIROS-T-0183,
/// amending KAIROS-A-0006). Matching is textual, so while it was called
/// `manage_members` the [`GLOB_MANAGE`] glob covered it — an admin granting
/// `manage_*` as shorthand for "the work-item capabilities" also handed over
/// the power to grant and revoke other people's capabilities. Renaming it out
/// of the prefix makes `manage_*` mean what it reads, without a second rule in
/// the matcher.
pub const ADMINISTER_MEMBERS: &str = "administer_members";
/// Publish and unpublish the sites of the repositories of the board's team
/// (KAIROS-T-0406, KAIROS-D-0012). Granted on the team's delivery board,
/// usually to the service account of a repository's CI. Not implied by team
/// membership, and no glob but `*` covers it.
pub const PAGES: &str = "pages";
/// Delete an archived task for good: a purge (KAIROS-T-0570). Granted on a
/// delivery board. Team membership does not imply it, and no glob but `*`
/// covers it. An organization admin holds it by the admin bypass. A purge
/// cannot be undone, so the right to archive (`manage_tasks`) is not the
/// right to purge.
pub const PURGE: &str = "purge";
/// Membership of a strategy or an initiative board (KAIROS-T-0541,
/// amending KAIROS-A-0025 decision 1). A grant of it gives the edit and the
/// move of each item on the board ([`STRATEGY_BOARD_MEMBER_CAPABILITIES`],
/// [`INITIATIVE_BOARD_MEMBER_CAPABILITIES`]). It applies on those two
/// levels only. No glob but `*` covers it. Until KAIROS-T-0541, ANY grant on
/// the board made a member, so a grant of `transition_items` or
/// `configure_boards` alone also edited each item.
pub const MEMBER: &str = "member";
/// Confirm or reject an edge proposal (KAIROS-T-0659, superseding the
/// decider rule of KAIROS-A-0056). A holder on the board of either end of
/// the proposed edge decides; an org admin decides each proposal. The
/// holder may be a person, the agent key of that person, or a service
/// account, and may be the proposer. It applies on each level. No glob but
/// `*` covers it, and no team or board membership implies it.
pub const APPROVE_PROPOSALS: &str = "approve_proposals";

/// Glob: all capabilities on the board (full access).
pub const GLOB_ALL: &str = "*";
/// Glob: all `manage_<entity>` capabilities — and, since KAIROS-T-0183
/// renamed [`ADMINISTER_MEMBERS`] out of the prefix, exactly those. It is a
/// genuine family glob now rather than one by appearance.
pub const GLOB_MANAGE: &str = "manage_*";
/// Glob: all `configure_*` capabilities — currently just
/// [`CONFIGURE_BOARDS`], since KAIROS-T-0182 removed the other two. Kept as a
/// glob rather than collapsed into the specific capability: it is a stable
/// grant an admin may already hold, and it keeps meaning "whatever configuring
/// this board comes to mean".
pub const GLOB_CONFIGURE: &str = "configure_*";
/// Glob: all `transition_*` capabilities (currently just
/// [`TRANSITION_ITEMS`], but future-proof).
pub const GLOB_TRANSITION: &str = "transition_*";

/// Every specific (non-glob) capability in the A-0006 vocabulary.
pub const CAPABILITIES: &[&str] = &[
    MANAGE_STRATEGIES,
    MANAGE_INITIATIVES,
    MANAGE_TASKS,
    MANAGE_DOCUMENTS,
    MANAGE_ADRS,
    TRANSITION_ITEMS,
    CONFIGURE_BOARDS,
    ADMINISTER_MEMBERS,
    PAGES,
    PURGE,
    MEMBER,
    APPROVE_PROPOSALS,
];

/// Every sanctioned glob form (trailing-`*` only, per A-0006).
pub const GLOBS: &[&str] = &[GLOB_ALL, GLOB_MANAGE, GLOB_CONFIGURE, GLOB_TRANSITION];

/// The capabilities IMPLIED by membership of a board's owning team
/// (KAIROS-T-0072 amendment to A-0006): day-to-day delivery work only.
/// Deliberately narrow — no `configure_*`, no `administer_members`, and none of
/// the strategy/initiative/ADR `manage_*` families. The one widening is
/// `manage_adrs` on the ADR board of the team
/// ([`TEAM_ADR_BOARD_IMPLIED_CAPABILITIES`], COLLIERY-T-3108). On strategy
/// and initiative boards, membership of the BOARD (any grant on it) gives
/// the edit and the move of its items
/// ([`STRATEGY_BOARD_MEMBER_CAPABILITIES`], KAIROS-T-0410). Since
/// KAIROS-T-0440 such a board has a team too, and the members of that team
/// get the same ([`team_capabilities`]). Since KAIROS-T-0541 the board
/// membership is the grant of [`MEMBER`], not any grant.
pub const TEAM_IMPLIED_CAPABILITIES: &[&str] = &[MANAGE_TASKS, MANAGE_DOCUMENTS, TRANSITION_ITEMS];

/// Does membership of the board's owning team satisfy `required` on its
/// own (KAIROS-T-0072)? Exact vocabulary membership — implied capabilities
/// are specific, never globs, so no `capability_matches` translation
/// applies here.
pub fn team_implies(required: &str) -> bool {
    TEAM_IMPLIED_CAPABILITIES.contains(&required)
}

/// The capabilities implied by membership of the team that owns an ADR
/// board (COLLIERY-T-3108): the delivery set plus `manage_adrs`. A team
/// writes its own delivery ADRs with no grant. The organization ADR board
/// has no team, so this never applies there: `manage_adrs` on it stays an
/// explicit grant.
pub const TEAM_ADR_BOARD_IMPLIED_CAPABILITIES: &[&str] = &[
    MANAGE_TASKS,
    MANAGE_DOCUMENTS,
    TRANSITION_ITEMS,
    MANAGE_ADRS,
];

/// Does membership of the team that owns an ADR board satisfy `required`
/// on that board (COLLIERY-T-3108)? Exact vocabulary membership, as for
/// [`team_implies`].
pub fn team_implies_on_adr_board(required: &str) -> bool {
    TEAM_ADR_BOARD_IMPLIED_CAPABILITIES.contains(&required)
}

/// The capabilities implied by membership of the team that owns a strategy
/// board (KAIROS-T-0440, KAIROS-A-0026): the delivery set plus what board
/// membership gives there ([`STRATEGY_BOARD_MEMBER_CAPABILITIES`]).
pub const TEAM_STRATEGY_BOARD_IMPLIED_CAPABILITIES: &[&str] = &[
    MANAGE_TASKS,
    MANAGE_DOCUMENTS,
    TRANSITION_ITEMS,
    MANAGE_STRATEGIES,
];

/// As [`TEAM_STRATEGY_BOARD_IMPLIED_CAPABILITIES`], for the team that owns
/// an initiative board.
pub const TEAM_INITIATIVE_BOARD_IMPLIED_CAPABILITIES: &[&str] = &[
    MANAGE_TASKS,
    MANAGE_DOCUMENTS,
    TRANSITION_ITEMS,
    MANAGE_INITIATIVES,
];

/// The capabilities that membership of the owning team gives on a board of
/// the level `board_level` (the text of `boards.board_level`): the delivery
/// set, widened on an ADR board (`manage_adrs`), a strategy board
/// (`manage_strategies`) and an initiative board (`manage_initiatives`).
pub fn team_capabilities(board_level: &str) -> &'static [&'static str] {
    match board_level {
        "adr" => TEAM_ADR_BOARD_IMPLIED_CAPABILITIES,
        "strategy" => TEAM_STRATEGY_BOARD_IMPLIED_CAPABILITIES,
        "initiative" => TEAM_INITIATIVE_BOARD_IMPLIED_CAPABILITIES,
        _ => TEAM_IMPLIED_CAPABILITIES,
    }
}

/// The capabilities implied by MEMBERSHIP of a strategy board
/// (KAIROS-T-0410, KAIROS-A-0025 decision 1): a principal who holds the
/// [`MEMBER`] grant on the board (KAIROS-T-0541; `member` or `*`) may edit
/// and move every strategy on it. No other grant makes a member: a grant of
/// `transition_items` moves and does not edit, and `configure_boards` or
/// `administer_members` give neither. Computed at check time from the rows
/// of `board_member_capabilities`, never stored, so the revoke of the
/// member grant is the revocation.
///
/// Archive and restore are NOT given by this set (decision 8). They are
/// edits by the edit rule, and `manage_<type>` is the capability of both,
/// so the rule reads the grants for them without this implication
/// ([`EditUse::Archive`]). Nor `configure_boards` or `administer_members`.
pub const STRATEGY_BOARD_MEMBER_CAPABILITIES: &[&str] = &[MANAGE_STRATEGIES, TRANSITION_ITEMS];

/// The capabilities implied by membership of an initiative board
/// (KAIROS-T-0410, KAIROS-A-0025 decision 1): as
/// [`STRATEGY_BOARD_MEMBER_CAPABILITIES`], for initiatives.
pub const INITIATIVE_BOARD_MEMBER_CAPABILITIES: &[&str] = &[MANAGE_INITIATIVES, TRANSITION_ITEMS];

/// Does membership of a STRATEGY board (the [`MEMBER`] grant) satisfy
/// `required` on that board (KAIROS-T-0410)? Exact vocabulary membership,
/// as for [`team_implies`].
pub fn member_implies_on_strategy_board(required: &str) -> bool {
    STRATEGY_BOARD_MEMBER_CAPABILITIES.contains(&required)
}

/// Does membership of an INITIATIVE board (the [`MEMBER`] grant) satisfy
/// `required` on that board (KAIROS-T-0410)? Exact vocabulary membership,
/// as for [`team_implies`].
pub fn member_implies_on_initiative_board(required: &str) -> bool {
    INITIATIVE_BOARD_MEMBER_CAPABILITIES.contains(&required)
}

/// The capabilities that membership gives on a board of the level
/// `board_level` (the text of `boards.board_level`): the strategy set, the
/// initiative set, or none (delivery and ADR boards, KAIROS-T-0410).
pub fn board_member_capabilities(board_level: &str) -> &'static [&'static str] {
    match board_level {
        "strategy" => STRATEGY_BOARD_MEMBER_CAPABILITIES,
        "initiative" => INITIATIVE_BOARD_MEMBER_CAPABILITIES,
        _ => &[],
    }
}

/// Do these stored grants of one principal on one board make it a member
/// of the board (KAIROS-T-0541)? Only a grant that matches [`MEMBER`] does:
/// `member` itself, or `*`. The pure mirror of the third arm of the SQL
/// check in `kairos-db::abac`.
pub fn grants_membership<S: AsRef<str>>(grants: &[S]) -> bool {
    grants
        .iter()
        .any(|grant| capability_matches(grant.as_ref(), MEMBER))
}

/// The capabilities that these stored grants of one principal give by
/// membership on a board of the level `board_level` (KAIROS-T-0541): the
/// set of [`board_member_capabilities`] when the grants hold [`MEMBER`]
/// ([`grants_membership`]), else none. `whoami` reports this set.
pub fn member_capabilities_of_grants<S: AsRef<str>>(
    board_level: &str,
    grants: &[S],
) -> &'static [&'static str] {
    if grants_membership(grants) {
        board_member_capabilities(board_level)
    } else {
        &[]
    }
}

/// The board levels (the text of `boards.board_level`), in the order that
/// a page shows them: strategy, initiative, delivery, ADR (KAIROS-T-0413).
pub const BOARD_LEVELS: &[&str] = &["strategy", "initiative", "delivery", "adr"];

/// The grantable capabilities that APPLY on a board of the level
/// `board_level` (KAIROS-T-0413): the capabilities that authorize a write
/// that a board of that level can have. A board holds one item type and
/// documents (`types_of_level` of the server), so each level has one
/// `manage_<type>`, and `manage_documents`, `transition_items`,
/// `configure_boards`, `administer_members` and `approve_proposals`
/// (KAIROS-T-0659). An unknown level has none.
///
/// The ONE map of levels to capabilities: the server refuses a grant of a
/// different capability in the change of the grants of one person over
/// many boards, and sends this list to the GUI for each board, which
/// enables only these cells of the grid.
pub fn capabilities_of_level(board_level: &str) -> &'static [&'static str] {
    const STRATEGY: &[&str] = &[
        MANAGE_STRATEGIES,
        MANAGE_DOCUMENTS,
        TRANSITION_ITEMS,
        CONFIGURE_BOARDS,
        ADMINISTER_MEMBERS,
        MEMBER,
        APPROVE_PROPOSALS,
    ];
    const INITIATIVE: &[&str] = &[
        MANAGE_INITIATIVES,
        MANAGE_DOCUMENTS,
        TRANSITION_ITEMS,
        CONFIGURE_BOARDS,
        ADMINISTER_MEMBERS,
        MEMBER,
        APPROVE_PROPOSALS,
    ];
    const DELIVERY: &[&str] = &[
        MANAGE_TASKS,
        MANAGE_DOCUMENTS,
        TRANSITION_ITEMS,
        CONFIGURE_BOARDS,
        ADMINISTER_MEMBERS,
        PAGES,
        PURGE,
        APPROVE_PROPOSALS,
    ];
    const ADR: &[&str] = &[
        MANAGE_ADRS,
        MANAGE_DOCUMENTS,
        TRANSITION_ITEMS,
        CONFIGURE_BOARDS,
        ADMINISTER_MEMBERS,
        APPROVE_PROPOSALS,
    ];
    match board_level {
        "strategy" => STRATEGY,
        "initiative" => INITIATIVE,
        "delivery" => DELIVERY,
        "adr" => ADR,
        _ => &[],
    }
}

/// A request to any team (KAIROS-T-0105, amended by COLLIERY-T-0218 for
/// COLLIERY-A-0023 decisions 7 and 8): any member of the tenant may send a
/// request to any team: a TASK in the entry column of its delivery board,
/// in the support lane. Teams request work of each other; no team pushes
/// work to a different team. The repository is optional and is not part of
/// the condition. Until COLLIERY-T-0218 it was: the task had to link to a
/// repository, and the lane was whatever the caller sent. The name is kept,
/// though the entry column need not be called Backlog. COMPUTED, never
/// stored: it is not in the
/// grantable vocabulary ([`CAPABILITIES`]), so `board_member_capabilities`
/// can never carry it, and it is satisfied purely by tenant membership on
/// a delivery board. Nothing past the entry column is opened by it.
pub const FILE_BACKLOG: &str = "file_backlog";

/// Every capability that is COMPUTED rather than granted — never in the
/// grantable vocabulary (so grant/revoke refuse it), reported by `whoami`
/// under `implicit`.
pub const COMPUTED_CAPABILITIES: &[&str] = &[FILE_BACKLOG];

/// The structure capability (KAIROS-T-0337, amending KAIROS-A-0006 and
/// KAIROS-A-0017): the work of an organization admin on the structure of
/// the organization, and no more. An organization admin grants it to a
/// person or to a service account, at one of two scopes:
///
/// - **the organization**: teams, team members, boards, the columns, the
///   transitions, the members and the grants of each board, and the
///   repositories;
/// - **one board**: the columns, the transitions, the members and the
///   grants of that board ([`STRUCTURE_COVERED_CAPABILITIES`]). Not new
///   teams, not other boards.
///
/// Accounts, API keys, SCIM tokens, the deployment admins and the tenant
/// stay with the organization admin.
///
/// It is NOT in [`CAPABILITIES`], and `board_member_capabilities` never
/// holds it: the grants are rows of the table `structure_grants`, and the
/// check reads them by name. So no glob covers it. A holder of `manage_*`
/// or of `*` on a board does not hold it (KAIROS-T-0183: the name has the
/// `manage_` prefix, and the glob of that family must not reach board
/// administration).
pub const MANAGE_STRUCTURE: &str = "manage_structure";

/// The board capabilities that a grant of [`MANAGE_STRUCTURE`] satisfies on
/// its board (a board grant), or on each board (an organization grant).
pub const STRUCTURE_COVERED_CAPABILITIES: &[&str] = &[CONFIGURE_BOARDS, ADMINISTER_MEMBERS];

/// Does a grant of [`MANAGE_STRUCTURE`] satisfy the board capability
/// `required`? Exact vocabulary membership, as for [`team_implies`].
pub fn structure_covers(required: &str) -> bool {
    STRUCTURE_COVERED_CAPABILITIES.contains(&required)
}

// ---------------------------------------------------------------------------
// The edit rule and the link rule (COLLIERY-T-0228)
// ---------------------------------------------------------------------------

/// What the server knows about one principal and one item when it decides
/// an edit (COLLIERY-T-0228). The server loads the four facts; the
/// decision is [`may_edit_item`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditFacts {
    /// The principal created the item (`created_by`).
    pub created_item: bool,
    /// The principal holds `manage_<type>` on the authorization board of
    /// the item. `false` when the item has no authorization board.
    pub holds_manage: bool,
    /// The principal is an admin of the organization.
    pub is_org_admin: bool,
    /// The item is a task, and the principal has its claim (KAIROS-T-0532):
    /// the person who works it, also through an agent key. `false` for an
    /// archive or a restore ([`EditUse::Archive`]).
    pub holds_claim: bool,
}

/// What an edit is for, when the server loads [`EditFacts`]
/// (KAIROS-T-0410, KAIROS-A-0025 decision 8). It decides whether
/// membership of a strategy or initiative board
/// ([`member_implies_on_strategy_board`],
/// [`member_implies_on_initiative_board`]) counts toward `holds_manage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditUse {
    /// The title, the content, the metadata, the links, the team: each
    /// edit that is not an archive. Membership of the board counts.
    Edit,
    /// An archive (with its cascade) or a restore. Only a grant counts (and
    /// the team implication of delivery and ADR boards, as before): a
    /// member of a strategy or initiative board with no `manage_<type>`
    /// grant cannot archive or restore what it did not create.
    Archive,
}

/// THE EDIT RULE (COLLIERY-T-0228). A principal, a person or a service
/// account, may edit an item when ONE of these is true:
///
/// 1. the principal created the item,
/// 2. the principal holds `manage_<type>` on the authorization board of the
///    item,
/// 3. the principal is an admin of the organization,
/// 4. the item is a task and the principal has its claim (KAIROS-T-0532),
///    for an edit that is not an archive or a restore.
///
/// Creation is the primary mechanism of ownership. A capability on a board
/// is how a team shares that ownership. The claim is the work: the person
/// who works a task writes its notes, its state and its metadata, also
/// when a different team owns the board. The rule applies to each item type:
/// strategy, initiative, task, document, ADR. It reads who created the
/// item and not where the item is, so the right stays with the creator when
/// the item moves to a different board.
///
/// An edit is: the title and the content, the metadata, the repository of a
/// task, the editorial lifecycle of a document, archive, and restore.
///
/// Creation does NOT grant movement. To move an item between columns, to
/// change its lane, or to move it to a different board, the principal needs
/// the capability on the board (`transition_items`, or `manage_tasks` on
/// the two boards of a move). A team controls its own plan
/// (COLLIERY-T-0218, COLLIERY-A-0023): a person who sends a request to a
/// team can correct its text, link it and archive it, and cannot put it in
/// the plan of that team. Creation grants nothing on a board, a team, a
/// member, a capability, a repository or the configuration of the tenant.
pub fn may_edit_item(facts: EditFacts) -> bool {
    facts.created_item || facts.holds_manage || facts.is_org_admin || facts.holds_claim
}

/// The `manage_<type>` capability of an item type: the capability of the
/// edit rule ([`may_edit_item`]). One function for the server and for the
/// cascade of an archive (COLLIERY-T-0234), so the refusal of an edit and
/// the reason of an archive name the same capability.
pub fn manage_capability(item_type: crate::short_code::ItemType) -> &'static str {
    use crate::short_code::ItemType;
    match item_type {
        ItemType::Strategy => MANAGE_STRATEGIES,
        ItemType::Initiative => MANAGE_INITIATIVES,
        ItemType::Task => MANAGE_TASKS,
        ItemType::Document => MANAGE_DOCUMENTS,
        ItemType::Adr => MANAGE_ADRS,
    }
}

/// THE LINK RULE (COLLIERY-T-0228). A principal may write an edge, which
/// is to create it or to remove it, when the principal may edit the item
/// at EITHER end ([`may_edit_item`]). The rule is the same for each
/// relationship type: `parent`, `blocks`, `supports`, `informs`,
/// `supersedes`, `amends`.
///
/// Until COLLIERY-T-0228 `supports`, `informs` and `supersedes` needed the
/// admin role, and `parent` and `blocks` looked at the creator of the
/// source only. One rule for each type is easier to learn, and a person
/// who can edit an item could not say what the item relates to.
///
/// The rule decides WHO. It does not decide WHICH edges can exist: the
/// type rules (`graph::check_link`), the cycle check and the duplicate
/// check do not change.
///
/// One exception since KAIROS-T-0569: the CREATE of a `blocks` edge needs
/// the target ([`edge_create`]).
pub fn may_write_edge(may_edit_source: bool, may_edit_target: bool) -> bool {
    may_edit_source || may_edit_target
}

/// What the create of an edge does (KAIROS-T-0569): the answer of
/// [`edge_create`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeCreate {
    /// The principal writes the edge.
    Link,
    /// The principal may not write the edge, and the call records a
    /// proposal of it. A decider confirms it (KAIROS-A-0060).
    Propose,
    /// The principal may not write the edge, nor propose it by this call.
    Refuse,
}

/// THE LINK RULE FOR THE CREATE OF AN EDGE (KAIROS-T-0569).
///
/// A `blocks` edge changes the work of the TARGET: its card shows it
/// blocked, and its claimant gets the events. So the principal must be
/// able to edit the target. A principal who can edit the source only
/// gets a proposal, which a decider confirms or rejects. Each other type
/// keeps the rule of [`may_write_edge`]: either end.
///
/// The remove of an edge keeps [`may_write_edge`] for each type, `blocks`
/// too. To remove a blocker is not to block the work of a different team.
pub fn edge_create(
    relationship: crate::graph::Relationship,
    may_edit_source: bool,
    may_edit_target: bool,
) -> EdgeCreate {
    match relationship {
        crate::graph::Relationship::Blocks if may_edit_target => EdgeCreate::Link,
        crate::graph::Relationship::Blocks if may_edit_source => EdgeCreate::Propose,
        crate::graph::Relationship::Blocks => EdgeCreate::Refuse,
        _ if may_write_edge(may_edit_source, may_edit_target) => EdgeCreate::Link,
        _ => EdgeCreate::Refuse,
    }
}

/// May this principal REMOVE a `supports` edge that points at a document
/// (COLLIERY-T-0235)? Only a principal that may edit the DOCUMENT. The
/// right to edit the parent is not sufficient.
///
/// What a document supports is a statement of its owners: a principal
/// that can edit the parent and not the document cannot take that
/// statement away. Until COLLIERY-T-3109 a document could have no board,
/// and its earliest parent gave it its authority, so the remove of a
/// parent could also move the authority. Each document has an owner board
/// now, and a `supports` edge gives no authority, but the rule of WHO
/// stays.
pub fn may_unlink_document_parent(may_edit_document: bool) -> bool {
    may_edit_document
}

// ---------------------------------------------------------------------------
// Tenant-wide configuration policy (A-0006 "items not on boards")
// ---------------------------------------------------------------------------

/// Tenant-wide configuration resources that live on no board. Per A-0006
/// these are org-admin-only: no board-scoped capability can ever authorize
/// writes to them.
///
/// Relationships were in this list until COLLIERY-T-0228. An edge is not
/// configuration of the tenant: it is a statement about two items, so the
/// person who may edit one of those items may write it. The server decides
/// that (`require_edge_write`), for each relationship type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TenantConfigResource {
    /// `templates` rows.
    Templates,
    /// `metadata_definitions` (and their enum options).
    MetadataDefinitions,
}

impl TenantConfigResource {
    /// Whether writes to this resource require `organization_members.role =
    /// 'admin'`. Constant `true` for every variant — encoded as a function so
    /// the policy has one citable home (A-0006: "Templates, metadata
    /// definitions … only org admins can create, modify, or delete them").
    pub const fn org_admin_only(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Glob matching
// ---------------------------------------------------------------------------

/// Does the stored grant `granted` satisfy the `required` capability?
///
/// Implements exactly the A-0006 SQL LIKE translation (module docs): `*` in
/// `granted` is a multi-character wildcard, everything else — including
/// LIKE's own `%`/`_`/`\`, which the SQL escapes — is literal, and a bare
/// `*` grants all. Degenerate inputs fall out of the same rules: an empty
/// grant matches only an empty required string (`'' LIKE ''` is true), and
/// an empty required string matches only grants whose literal parts are all
/// empty (`''`, `*`, `**`, …).
pub fn capability_matches(granted: &str, required: &str) -> bool {
    if granted == GLOB_ALL {
        // The SQL's explicit `capability = '*'` arm; also subsumed by the
        // pattern arm ('%' matches anything), kept for exact parity.
        return true;
    }
    glob_match(granted, required)
}

/// LIKE-style match: `*` in `pattern` matches any (possibly empty) sequence;
/// every other character matches itself.
fn glob_match(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().expect("split yields at least one part");
    let Some(mut remaining) = text.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        // No '*' in the pattern: exact match.
        return remaining.is_empty();
    };
    // Each middle part must appear in order; '*' absorbs the gaps.
    for part in middle {
        match remaining.find(part) {
            Some(idx) => remaining = &remaining[idx + part.len()..],
            None => return false,
        }
    }
    remaining.ends_with(last)
}

/// Who decides an edge proposal (KAIROS-T-0659): an org admin, or a holder
/// of [`APPROVE_PROPOSALS`] on the board of the source or of the target.
/// One end is sufficient. Who the caller is does not count: a person, an
/// agent key, a service account and the proposer all take this rule.
pub fn may_decide_proposal(is_org_admin: bool, on_source: bool, on_target: bool) -> bool {
    is_org_admin || on_source || on_target
}

/// The A-0006 decision over a user's loaded grants for one board: allowed
/// iff any grant matches the required capability (whitelist — no grants, no
/// access).
pub fn is_authorized(grants: &[String], required: &str) -> bool {
    grants
        .iter()
        .any(|granted| capability_matches(granted, required))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- the structure capability (KAIROS-T-0337) ------------------------------

    /// `manage_structure` covers the configuration and the members of a
    /// board, and no work-item capability. It is not a board capability, so
    /// no grant of the board vocabulary can carry it.
    #[test]
    fn manage_structure_covers_board_configuration_and_members_only() {
        assert!(structure_covers(CONFIGURE_BOARDS));
        assert!(structure_covers(ADMINISTER_MEMBERS));
        for capability in [
            MANAGE_TASKS,
            MANAGE_DOCUMENTS,
            MANAGE_ADRS,
            MANAGE_STRATEGIES,
            MANAGE_INITIATIVES,
            TRANSITION_ITEMS,
            FILE_BACKLOG,
            MANAGE_STRUCTURE,
        ] {
            assert!(!structure_covers(capability), "{capability}");
        }
        assert!(!CAPABILITIES.contains(&MANAGE_STRUCTURE));
        assert!(!GLOBS.contains(&MANAGE_STRUCTURE));
    }

    // -- the parent of a document (COLLIERY-T-0235) ----------------------------

    #[test]
    fn the_remove_of_a_parent_needs_the_right_to_edit_the_document() {
        assert!(may_unlink_document_parent(true));
        assert!(!may_unlink_document_parent(false));
    }

    // -- exact matches ---------------------------------------------------------

    #[test]
    fn exact_capability_matches_itself_only() {
        for capability in CAPABILITIES {
            assert!(
                capability_matches(capability, capability),
                "{capability} must match itself"
            );
        }
        assert!(!capability_matches(MANAGE_TASKS, MANAGE_DOCUMENTS));
        assert!(!capability_matches(MANAGE_TASKS, "manage_task"));
        assert!(!capability_matches(MANAGE_TASKS, "manage_taskss"));
        // Matching is case-sensitive, like LIKE.
        assert!(!capability_matches(MANAGE_TASKS, "MANAGE_TASKS"));
    }

    // -- '*' grants everything ---------------------------------------------------

    #[test]
    fn bare_star_grants_every_capability() {
        for capability in CAPABILITIES {
            assert!(capability_matches(GLOB_ALL, capability));
        }
        // '%' matches even the empty string in SQL; mirrored here.
        assert!(capability_matches(GLOB_ALL, ""));
        assert!(capability_matches(GLOB_ALL, "anything_at_all"));
    }

    // -- prefix globs -------------------------------------------------------------

    #[test]
    fn manage_glob_matches_manage_capabilities_only() {
        for capability in [
            MANAGE_STRATEGIES,
            MANAGE_INITIATIVES,
            MANAGE_TASKS,
            MANAGE_DOCUMENTS,
            MANAGE_ADRS,
        ] {
            assert!(
                capability_matches(GLOB_MANAGE, capability),
                "manage_* must cover {capability}"
            );
        }
        assert!(!capability_matches(GLOB_MANAGE, TRANSITION_ITEMS));
        assert!(!capability_matches(GLOB_MANAGE, CONFIGURE_BOARDS));
        // 'manage_tasks' LIKE 'manage_%' semantics: the bare prefix matches too.
        assert!(capability_matches(GLOB_MANAGE, "manage_"));
        // But not a non-prefix ('manage' is shorter than 'manage_').
        assert!(!capability_matches(GLOB_MANAGE, "manage"));
    }

    /// KAIROS-T-0183, asserted BY NAME rather than left to be derived from the
    /// matching rule: board administration is not part of the `manage_*` family.
    ///
    /// Before the rename this was the reverse, and asserted as such, so the
    /// behaviour looked intentional whatever anyone intended. The decision
    /// recorded here is that granting `manage_*` must NOT hand over the power to
    /// grant capabilities to others.
    ///
    /// Note that the matcher itself is unchanged and still a plain textual
    /// translation of A-0006's `LIKE` â which is why this test is about the
    /// vocabulary rather than about `capability_matches`.
    #[test]
    fn manage_glob_does_not_confer_board_administration() {
        assert!(!capability_matches(GLOB_MANAGE, ADMINISTER_MEMBERS));
        assert!(!is_authorized(
            &[GLOB_MANAGE.to_string()],
            ADMINISTER_MEMBERS
        ));
        // It takes its own grant, or `*`.
        assert!(is_authorized(
            &[ADMINISTER_MEMBERS.to_string()],
            ADMINISTER_MEMBERS
        ));
        assert!(is_authorized(&[GLOB_ALL.to_string()], ADMINISTER_MEMBERS));
        // And the old name is not a back door: it is out of the vocabulary, so
        // a stored grant of it matches nothing the API will ever require.
        assert!(!CAPABILITIES.contains(&"manage_members"));
    }

    #[test]
    fn configure_and_transition_globs() {
        // Textual, not vocabulary-aware: the glob matches any `configure_`
        // prefix, including capabilities that do not exist. That is what the
        // A-0006 LIKE translation does, and KAIROS-T-0182 shrinking the
        // vocabulary to one member must not quietly change it.
        for capability in [CONFIGURE_BOARDS, "configure_boards_someday"] {
            assert!(capability_matches(GLOB_CONFIGURE, capability));
        }
        assert!(!capability_matches(GLOB_CONFIGURE, MANAGE_TASKS));
        assert!(capability_matches(GLOB_TRANSITION, TRANSITION_ITEMS));
        assert!(!capability_matches(GLOB_TRANSITION, MANAGE_TASKS));
        assert!(!capability_matches(GLOB_TRANSITION, CONFIGURE_BOARDS));
    }

    #[test]
    fn non_matching_prefixes_are_rejected() {
        assert!(!capability_matches("manag_*", MANAGE_TASKS));
        assert!(!capability_matches("xmanage_*", MANAGE_TASKS));
        assert!(!capability_matches("manage_t*", MANAGE_DOCUMENTS));
        // A specific grant never satisfies a glob-shaped requirement.
        assert!(!capability_matches(MANAGE_TASKS, GLOB_MANAGE));
    }

    // -- empty strings --------------------------------------------------------------

    #[test]
    fn empty_strings_mirror_sql_like() {
        // '' LIKE '' is true; degenerate but harmless (neither should exist).
        assert!(capability_matches("", ""));
        // '' LIKE 'manage\_%' is false.
        assert!(!capability_matches(GLOB_MANAGE, ""));
        assert!(!capability_matches(MANAGE_TASKS, ""));
        // 'manage_tasks' LIKE '' is false.
        assert!(!capability_matches("", MANAGE_TASKS));
        // '' LIKE '%' is true (bare star).
        assert!(capability_matches(GLOB_ALL, ""));
    }

    // -- hostile inputs: LIKE metacharacters stored in grants --------------------------

    #[test]
    fn stored_percent_is_literal_not_wildcard() {
        // The SQL escapes '%' in stored capabilities; mirrored here: it only
        // matches itself.
        assert!(!capability_matches("manage%", "manage_tasks"));
        assert!(!capability_matches("manage%", "manageX"));
        assert!(capability_matches("manage%", "manage%"));
        assert!(!capability_matches("%", MANAGE_TASKS));
        assert!(capability_matches("%", "%"));
    }

    #[test]
    fn stored_underscore_is_literal_not_single_char_wildcard() {
        // '_' in LIKE matches any ONE char; escaped in SQL, literal here.
        assert!(!capability_matches("manage_tasks", "manageXtasks"));
        assert!(capability_matches("manage_tasks", "manage_tasks"));
        assert!(!capability_matches("_", "x"));
        assert!(capability_matches("_", "_"));
    }

    #[test]
    fn stored_backslash_is_literal() {
        // '\' is LIKE's escape char; escaped in SQL, literal here.
        assert!(capability_matches("manage\\tasks", "manage\\tasks"));
        assert!(!capability_matches("manage\\tasks", "manage_tasks"));
        assert!(!capability_matches("manage\\_tasks", "manage_tasks"));
    }

    #[test]
    fn embedded_star_wildcards_like_the_sql_translation() {
        // Mid-string '*' is OUTSIDE the sanctioned vocabulary (trailing only)
        // but the matcher mirrors SQL truth: replace(capability,'*','%')
        // translates every '*'.
        assert!(capability_matches("manage_*s", "manage_tasks"));
        assert!(capability_matches("manage_*s", "manage_s"));
        assert!(!capability_matches("manage_*s", "manage_tasks_x"));
        assert!(capability_matches("*_tasks", "manage_tasks"));
        assert!(!capability_matches("*_tasks", "manage_adrs"));
        assert!(capability_matches("m*_*s", "manage_tasks"));
        // '**' behaves like '%%' — same as one wildcard.
        assert!(capability_matches("manage_**", "manage_tasks"));
        assert!(capability_matches("**", ""));
    }

    // -- decision helper -----------------------------------------------------------

    #[test]
    fn is_authorized_is_a_whitelist_over_grants() {
        let grants = vec![MANAGE_TASKS.to_string(), GLOB_CONFIGURE.to_string()];
        assert!(is_authorized(&grants, MANAGE_TASKS));
        assert!(is_authorized(&grants, CONFIGURE_BOARDS));
        assert!(!is_authorized(&grants, MANAGE_DOCUMENTS));
        assert!(!is_authorized(&grants, TRANSITION_ITEMS));
        // No grants, no access.
        assert!(!is_authorized(&[], MANAGE_TASKS));
        // A '*' grant anywhere in the list authorizes everything.
        let star = vec![MANAGE_TASKS.to_string(), GLOB_ALL.to_string()];
        assert!(is_authorized(&star, TRANSITION_ITEMS));
    }

    // -- tenant-wide config policy ----------------------------------------------------

    #[test]
    fn tenant_config_resources_are_org_admin_only() {
        for resource in [
            TenantConfigResource::Templates,
            TenantConfigResource::MetadataDefinitions,
        ] {
            assert!(resource.org_admin_only());
        }
    }

    /// KAIROS-T-0072: team membership implies exactly the day-to-day
    /// delivery capabilities — and nothing configuration- or
    /// membership-shaped.
    #[test]
    fn team_implies_only_the_delivery_set() {
        assert!(team_implies(MANAGE_TASKS));
        assert!(team_implies(MANAGE_DOCUMENTS));
        assert!(team_implies(TRANSITION_ITEMS));

        assert!(!team_implies(MANAGE_STRATEGIES));
        assert!(!team_implies(MANAGE_INITIATIVES));
        assert!(!team_implies(MANAGE_ADRS));
        assert!(!team_implies(CONFIGURE_BOARDS));
        assert!(!team_implies(ADMINISTER_MEMBERS));
        // Globs are grant-side forms, never implied requirements.
        assert!(!team_implies(GLOB_ALL));
        assert!(!team_implies(GLOB_MANAGE));
    }

    /// COLLIERY-T-3108: on the ADR board of the team, membership also
    /// implies `manage_adrs` — and still nothing configuration- or
    /// membership-shaped.
    /// KAIROS-T-0440: the team of a strategy or initiative board gets what
    /// board membership gives there, and nothing configuration- or
    /// membership-shaped.
    #[test]
    fn team_capabilities_follow_the_level() {
        assert_eq!(team_capabilities("delivery"), TEAM_IMPLIED_CAPABILITIES);
        assert!(team_capabilities("adr").contains(&MANAGE_ADRS));
        for level in ["strategy", "initiative"] {
            for member in board_member_capabilities(level) {
                assert!(
                    team_capabilities(level).contains(member),
                    "{level}: {member}"
                );
            }
            assert!(!team_capabilities(level).contains(&CONFIGURE_BOARDS));
            assert!(!team_capabilities(level).contains(&ADMINISTER_MEMBERS));
        }
        assert!(!team_capabilities("strategy").contains(&MANAGE_INITIATIVES));
        assert!(!team_capabilities("initiative").contains(&MANAGE_STRATEGIES));
    }

    #[test]
    fn team_implies_on_adr_board_adds_manage_adrs_only() {
        for implied in TEAM_IMPLIED_CAPABILITIES {
            assert!(team_implies_on_adr_board(implied), "{implied:?}");
        }
        assert!(team_implies_on_adr_board(MANAGE_ADRS));

        assert!(!team_implies_on_adr_board(MANAGE_STRATEGIES));
        assert!(!team_implies_on_adr_board(MANAGE_INITIATIVES));
        assert!(!team_implies_on_adr_board(CONFIGURE_BOARDS));
        assert!(!team_implies_on_adr_board(ADMINISTER_MEMBERS));
        assert!(!team_implies_on_adr_board(GLOB_ALL));
        assert!(!team_implies_on_adr_board(GLOB_MANAGE));
    }

    /// KAIROS-T-0410: membership of a strategy or initiative board gives the
    /// `manage_<type>` of the board and `transition_items`, and nothing
    /// configuration- or membership-shaped. Other levels give nothing.
    #[test]
    fn board_membership_implies_edit_and_move_of_its_level_only() {
        assert!(member_implies_on_strategy_board(MANAGE_STRATEGIES));
        assert!(member_implies_on_strategy_board(TRANSITION_ITEMS));
        assert!(member_implies_on_initiative_board(MANAGE_INITIATIVES));
        assert!(member_implies_on_initiative_board(TRANSITION_ITEMS));

        assert!(!member_implies_on_strategy_board(MANAGE_INITIATIVES));
        assert!(!member_implies_on_initiative_board(MANAGE_STRATEGIES));
        for implies in [
            member_implies_on_strategy_board as fn(&str) -> bool,
            member_implies_on_initiative_board,
        ] {
            for capability in [
                MANAGE_TASKS,
                MANAGE_DOCUMENTS,
                MANAGE_ADRS,
                CONFIGURE_BOARDS,
                ADMINISTER_MEMBERS,
                FILE_BACKLOG,
                MANAGE_STRUCTURE,
                GLOB_ALL,
                GLOB_MANAGE,
            ] {
                assert!(!implies(capability), "{capability}");
            }
        }
        assert_eq!(
            board_member_capabilities("strategy"),
            STRATEGY_BOARD_MEMBER_CAPABILITIES
        );
        assert_eq!(
            board_member_capabilities("initiative"),
            INITIATIVE_BOARD_MEMBER_CAPABILITIES
        );
        assert!(board_member_capabilities("delivery").is_empty());
        assert!(board_member_capabilities("adr").is_empty());
    }

    /// KAIROS-T-0541: only the member grant (`member` or `*`) makes a
    /// member. `pages`, `transition_items`, `configure_boards`,
    /// `administer_members`, `manage_*` and `transition_*` do not.
    #[test]
    fn only_the_member_grant_makes_a_member() {
        assert!(grants_membership(&[MEMBER]));
        assert!(grants_membership(&[GLOB_ALL]));
        assert!(grants_membership(&[TRANSITION_ITEMS, MEMBER]));
        assert!(!grants_membership::<&str>(&[]));
        for grant in [
            PAGES,
            TRANSITION_ITEMS,
            CONFIGURE_BOARDS,
            ADMINISTER_MEMBERS,
            MANAGE_STRATEGIES,
            MANAGE_INITIATIVES,
            MANAGE_DOCUMENTS,
            GLOB_MANAGE,
            GLOB_CONFIGURE,
            GLOB_TRANSITION,
        ] {
            assert!(!grants_membership(&[grant]), "{grant}");
        }
        assert_eq!(
            member_capabilities_of_grants("initiative", &[MEMBER]),
            INITIATIVE_BOARD_MEMBER_CAPABILITIES
        );
        assert_eq!(
            member_capabilities_of_grants("strategy", &["*".to_string()]),
            STRATEGY_BOARD_MEMBER_CAPABILITIES
        );
        assert!(member_capabilities_of_grants("initiative", &[PAGES]).is_empty());
        assert!(member_capabilities_of_grants("strategy", &[TRANSITION_ITEMS]).is_empty());
        assert!(member_capabilities_of_grants("delivery", &[MEMBER]).is_empty());
        assert!(member_capabilities_of_grants("adr", &[MEMBER]).is_empty());
    }

    /// KAIROS-T-0413: each level has the `manage_<type>` of its item type
    /// and the five capabilities that each board has. Each applicable
    /// capability is grantable, and each grantable capability applies on
    /// some level.
    #[test]
    fn the_capabilities_of_each_level() {
        let common = [
            MANAGE_DOCUMENTS,
            TRANSITION_ITEMS,
            CONFIGURE_BOARDS,
            ADMINISTER_MEMBERS,
            APPROVE_PROPOSALS,
        ];
        for (level, own) in [
            ("strategy", MANAGE_STRATEGIES),
            ("initiative", MANAGE_INITIATIVES),
            ("delivery", MANAGE_TASKS),
            ("adr", MANAGE_ADRS),
        ] {
            let applies = capabilities_of_level(level);
            assert!(applies.contains(&own), "{level}");
            for capability in common {
                assert!(applies.contains(&capability), "{level} {capability}");
            }
            // KAIROS-T-0406: `pages` and KAIROS-T-0570: `purge` on delivery
            // boards; KAIROS-T-0541: `member` on strategy and initiative boards.
            let extra = match level {
                "delivery" => 2,
                "strategy" | "initiative" => 1,
                _ => 0,
            };
            // KAIROS-T-0659: `approve_proposals` on each level.
            assert_eq!(applies.len(), 6 + extra, "{level}");
            assert!(applies.iter().all(|c| CAPABILITIES.contains(c)));
        }
        assert!(!capabilities_of_level("delivery").contains(&MANAGE_STRATEGIES));
        assert!(!capabilities_of_level("strategy").contains(&MANAGE_TASKS));
        // KAIROS-T-0406: sites are published from the team's delivery board.
        assert!(capabilities_of_level("delivery").contains(&PAGES));
        assert!(!capabilities_of_level("initiative").contains(&PAGES));
        // KAIROS-T-0570: a task is purged on its delivery board.
        assert!(capabilities_of_level("delivery").contains(&PURGE));
        assert!(!capabilities_of_level("adr").contains(&PURGE));
        // KAIROS-T-0541: the member grant applies on strategy and
        // initiative boards only.
        assert!(capabilities_of_level("strategy").contains(&MEMBER));
        assert!(capabilities_of_level("initiative").contains(&MEMBER));
        assert!(!capabilities_of_level("delivery").contains(&MEMBER));
        assert!(!capabilities_of_level("adr").contains(&MEMBER));
        assert!(capabilities_of_level("team").is_empty());
        for capability in CAPABILITIES {
            assert!(
                BOARD_LEVELS
                    .iter()
                    .any(|level| capabilities_of_level(level).contains(capability)),
                "{capability} applies on no level"
            );
        }
    }

    /// COLLIERY-T-0228, KAIROS-T-0532: each of the four facts is
    /// sufficient, and with none of them there is no edit. All sixteen
    /// combinations.
    #[test]
    fn the_edit_rule_needs_one_fact_of_four() {
        for created_item in [false, true] {
            for holds_manage in [false, true] {
                for is_org_admin in [false, true] {
                    for holds_claim in [false, true] {
                        let facts = EditFacts {
                            created_item,
                            holds_manage,
                            is_org_admin,
                            holds_claim,
                        };
                        assert_eq!(
                            may_edit_item(facts),
                            created_item || holds_manage || is_org_admin || holds_claim,
                            "{facts:?}"
                        );
                    }
                }
            }
        }
        assert!(!may_edit_item(EditFacts {
            created_item: false,
            holds_manage: false,
            is_org_admin: false,
            holds_claim: false,
        }));
        // Creation alone: no capability, no role.
        assert!(may_edit_item(EditFacts {
            created_item: true,
            holds_manage: false,
            is_org_admin: false,
            holds_claim: false,
        }));
        // The claim alone (KAIROS-T-0532).
        assert!(may_edit_item(EditFacts {
            created_item: false,
            holds_manage: false,
            is_org_admin: false,
            holds_claim: true,
        }));
    }

    /// KAIROS-T-0659: the grant on one end is sufficient, and an org admin
    /// decides with no grant. With none of the three, the decision is
    /// refused.
    #[test]
    fn a_proposal_needs_the_grant_on_one_end_or_an_org_admin() {
        for is_org_admin in [false, true] {
            for on_source in [false, true] {
                for on_target in [false, true] {
                    assert_eq!(
                        may_decide_proposal(is_org_admin, on_source, on_target),
                        is_org_admin || on_source || on_target,
                        "{is_org_admin} {on_source} {on_target}"
                    );
                }
            }
        }
        assert!(!may_decide_proposal(false, false, false));
    }

    /// KAIROS-T-0659: only its own grant and `*` give `approve_proposals`.
    /// No family glob, no team and no board membership gives it.
    #[test]
    fn only_its_grant_or_star_approves_proposals() {
        assert!(is_authorized(
            &[APPROVE_PROPOSALS.to_string()],
            APPROVE_PROPOSALS
        ));
        assert!(is_authorized(&[GLOB_ALL.to_string()], APPROVE_PROPOSALS));
        for glob in GLOBS.iter().filter(|g| **g != GLOB_ALL) {
            assert!(!capability_matches(glob, APPROVE_PROPOSALS), "{glob}");
        }
        for level in BOARD_LEVELS {
            assert!(!team_capabilities(level).contains(&APPROVE_PROPOSALS));
            assert!(!board_member_capabilities(level).contains(&APPROVE_PROPOSALS));
            assert!(capabilities_of_level(level).contains(&APPROVE_PROPOSALS));
        }
        assert!(!is_authorized(
            &[MANAGE_TASKS.to_string(), MEMBER.to_string()],
            APPROVE_PROPOSALS
        ));
    }

    /// COLLIERY-T-0228: one end is sufficient; neither end is a refusal.
    #[test]
    fn the_link_rule_needs_one_end() {
        assert!(may_write_edge(true, true));
        assert!(may_write_edge(true, false));
        assert!(may_write_edge(false, true));
        assert!(!may_write_edge(false, false));
    }

    /// KAIROS-T-0569: a `blocks` edge needs the target. The source alone
    /// gives a proposal. Each other type needs one end.
    #[test]
    fn a_blocks_edge_needs_the_target() {
        use crate::graph::Relationship;
        assert_eq!(
            edge_create(Relationship::Blocks, true, true),
            EdgeCreate::Link
        );
        assert_eq!(
            edge_create(Relationship::Blocks, false, true),
            EdgeCreate::Link
        );
        assert_eq!(
            edge_create(Relationship::Blocks, true, false),
            EdgeCreate::Propose
        );
        assert_eq!(
            edge_create(Relationship::Blocks, false, false),
            EdgeCreate::Refuse
        );
        for relationship in [
            Relationship::Parent,
            Relationship::Supports,
            Relationship::Informs,
            Relationship::Supersedes,
            Relationship::Amends,
        ] {
            assert_eq!(edge_create(relationship, true, false), EdgeCreate::Link);
            assert_eq!(edge_create(relationship, false, true), EdgeCreate::Link);
            assert_eq!(edge_create(relationship, false, false), EdgeCreate::Refuse);
        }
    }

    /// COLLIERY-T-0228: creation is not a capability. It is not in the
    /// vocabulary that can be granted, nor in the computed one, so no grant
    /// and no `whoami` can carry it to a board.
    #[test]
    fn creation_is_not_a_capability() {
        for name in ["creator", "created_by", "owner", "edit_items"] {
            assert!(!CAPABILITIES.contains(&name));
            assert!(!COMPUTED_CAPABILITIES.contains(&name));
        }
    }

    #[test]
    fn file_backlog_is_computed_never_grantable_never_team_implied() {
        assert!(COMPUTED_CAPABILITIES.contains(&FILE_BACKLOG));
        // Not in the grantable vocabulary: no grant row can ever carry it,
        // and no family glob resolves to it (the bare `*` matches every
        // string by construction — moot, since a `*` holder already has
        // manage_tasks and the server never consults grants for it).
        assert!(!CAPABILITIES.contains(&FILE_BACKLOG));
        for glob in GLOBS.iter().filter(|g| **g != GLOB_ALL) {
            assert!(
                !capability_matches(glob, FILE_BACKLOG),
                "{glob} must not match {FILE_BACKLOG}"
            );
        }
        // Not implied by team membership either — it is tenant-wide.
        assert!(!team_implies(FILE_BACKLOG));
        for capability in CAPABILITIES {
            assert!(!COMPUTED_CAPABILITIES.contains(capability));
        }
    }

    /// KAIROS-T-0570: `purge` is a grant of its own. No team, ADR,
    /// strategy or initiative set implies it, no family glob covers it,
    /// and `*` does.
    #[test]
    fn purge_is_an_explicit_grant_that_membership_does_not_imply() {
        assert!(CAPABILITIES.contains(&PURGE));
        assert!(!team_implies(PURGE));
        assert!(!team_implies_on_adr_board(PURGE));
        for level in BOARD_LEVELS {
            assert!(!team_capabilities(level).contains(&PURGE), "{level}");
            assert!(
                !board_member_capabilities(level).contains(&PURGE),
                "{level}"
            );
        }
        assert!(!structure_covers(PURGE));
        for glob in GLOBS.iter().filter(|g| **g != GLOB_ALL) {
            assert!(!capability_matches(glob, PURGE), "{glob}");
        }
        assert!(!is_authorized(&[MANAGE_TASKS.to_string()], PURGE));
        assert!(is_authorized(&[PURGE.to_string()], PURGE));
        assert!(is_authorized(&[GLOB_ALL.to_string()], PURGE));
    }
}
