//! The documents and the ADRs that impact a repository, and the create of
//! a document for a repository (COLLIERY-T-0269, COLLIERY-I-0019).
//!
//! The vision of a repository is a document. It says why the repository
//! exists. It names the board of a team as its owner, and it impacts the
//! repository. [`RepositoryDocuments`] shows those documents for one
//! repository, and its dialog makes a new one with the two links:
//!
//! 1. `POST /api/documents` with `board`: the document, with its owner,
//! 2. `POST /api/documents/{code}/impacts`: the link to the repository.
//!
//! The two calls are not one transaction. When the second is refused, the
//! document is there with its owner, and the dialog says so with the short
//! code. The user adds the link on the page of the document.

use aurora_dark::components::{
    Alert, Anchor, Button, Empty, ErrorState, Group, Loading, Pill, Select, Stack, Text, TextInput,
};
use aurora_dark::frame::Modal;
use aurora_dark::tokens::{ApiError, token};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use super::api::{self, ImpactingItem};
use crate::auth::use_auth;
use crate::pages::boards;
use crate::pages::item::api as item_api;
use crate::pages::item::owner_board_targets;

/// The name of the template that the dialog selects first.
const PRODUCT_VISION: &str = "Product Vision";

/// The kind of an item that impacts a repository, for its pill: the
/// document type of a document that has one, else the entity type. Pure,
/// host-tested.
pub(crate) fn kind_label(item: &ImpactingItem) -> String {
    match (&item.document_type, item.entity_type.as_str()) {
        (Some(document_type), _) => document_type.clone(),
        (None, "adr") => "ADR".to_string(),
        (None, entity_type) => entity_type.to_string(),
    }
}

/// The state of an item that impacts a repository: the lifecycle of a
/// document, or the column of an ADR. Pure, host-tested.
pub(crate) fn state_label(item: &ImpactingItem) -> Option<String> {
    item.lifecycle.clone().or_else(|| item.column.clone())
}

/// The board that the dialog selects first: the board of the owner team
/// of the repository, when the user can give a document to it. Else the
/// first board of the list. `None` = the user can give a document to no
/// board. Pure, host-tested.
pub(crate) fn first_owner_board(
    targets: &[(String, String)],
    boards: &[boards::data::Board],
    owner_team: &str,
) -> Option<String> {
    boards
        .iter()
        // COLLIERY-T-3102: the delivery board of the team, not its ADR board.
        .filter(|board| board.board_level == "delivery")
        .filter(|board| board.team_id.as_deref() == Some(owner_team))
        .map(|board| &board.slug)
        .find(|slug| targets.iter().any(|(target, _)| target == *slug))
        .or_else(|| targets.first().map(|(slug, _)| slug))
        .cloned()
}

/// The documents and the ADRs that impact one repository. The list is
/// read when the user opens it.
#[component]
pub fn RepositoryDocuments(
    /// The slug of the repository.
    #[prop(into)]
    slug: String,
    /// The id of the owner team of the repository.
    #[prop(into)]
    owner_team: String,
) -> impl IntoView {
    let auth = use_auth();
    let slug = StoredValue::new(slug);
    let owner_team = StoredValue::new(owner_team);
    let open = RwSignal::new(false);
    let create_open = RwSignal::new(false);
    let reload = RwSignal::new(0u32);
    let items = LocalResource::new(move || {
        let _ = auth.token();
        let _ = reload.get();
        let wanted = open.get();
        async move {
            if wanted {
                api::impacted_by(auth, &slug.get_value()).await.map(Some)
            } else {
                Ok(None)
            }
        }
    });
    let retry = Callback::new(move |_| reload.update(|n| *n += 1));

    view! {
        <Stack gap="xs" attr:data-testid="repository-documents">
            <Group gap="xs">
                <Button variant="default" size="xs"
                    on_click=Callback::new(move |_| open.update(|open| *open = !*open))>
                    {move || if open.get() { "Hide documents" } else { "Show documents" }}
                </Button>
                <Button variant="default" size="xs"
                    on_click=Callback::new(move |_| create_open.set(true))>
                    "New document"
                </Button>
            </Group>
            <Show when=move || open.get()>
                {move || match items.get() {
                    None | Some(Ok(None)) => view! { <Loading label="Loading documents…"/> }.into_any(),
                    Some(Err(error)) => view! { <ErrorState error on_retry=retry/> }.into_any(),
                    Some(Ok(Some(items))) if items.is_empty() => view! {
                        <Empty message="No document and no ADR impacts this repository. To \
                                        add one, use the button New document."/>
                    }.into_any(),
                    Some(Ok(Some(items))) => view! {
                        <Stack gap="xs">
                            {items.into_iter().map(|item| {
                                let kind = kind_label(&item);
                                let state = state_label(&item);
                                let row = item.short_code.clone();
                                view! {
                                    <Group gap="sm" justify="between" attr:data-impacting=row>
                                        <Anchor href=format!("/items/{}", item.short_code)>
                                            {format!("{} — {}", item.short_code, item.title)}
                                        </Anchor>
                                        <Group gap="sm">
                                            {state.map(|state| view! {
                                                <Text size="xs" dimmed=true>{state}</Text>
                                            })}
                                            <Pill color=token::VIOLET>{kind}</Pill>
                                        </Group>
                                    </Group>
                                }
                            }).collect_view()}
                        </Stack>
                    }.into_any(),
                }}
            </Show>
            // COLLIERY-T-1836: an Aurora `Modal` (lg). It renders the form
            // only while it is open.
            <Modal
                open=create_open
                title=format!("New document for the repository {}", slug.get_value())
                size="lg"
            >
                <NewDocumentForm slug=slug.get_value() owner_team=owner_team.get_value()/>
            </Modal>
        </Stack>
    }
}

/// The form of the dialog. It is a component of its own, so its reads
/// exist only while the dialog is open.
#[component]
fn NewDocumentForm(#[prop(into)] slug: String, #[prop(into)] owner_team: String) -> impl IntoView {
    let auth = use_auth();
    let slug = StoredValue::new(slug);
    let owner_team = StoredValue::new(owner_team);
    let title = RwSignal::new(String::new());
    let template = RwSignal::new(String::new());
    let board = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let whoami = use_context::<LocalResource<Result<crate::api::Whoami, ApiError>>>();
    let templates = LocalResource::new(move || {
        let _ = auth.token();
        item_api::fetch_templates(auth)
    });
    let all_boards = LocalResource::new(move || {
        let _ = auth.token();
        boards::data::list_boards(auth)
    });
    let targets: Memo<Vec<(String, String)>> = Memo::new(move |_| {
        let me = whoami
            .and_then(|resource| resource.get())
            .and_then(Result::ok);
        let (Some(me), Some(Ok(list))) = (me, all_boards.get()) else {
            return Vec::new();
        };
        owner_board_targets(&me, &list, None)
    });
    // The first values of the two pickers, set from an Effect: never a
    // signal write in a tracked render (KAIROS-T-0114).
    Effect::new(move |_| {
        if let Some(Ok(list)) = templates.get()
            && template.get_untracked().is_empty()
        {
            let first = list
                .iter()
                .find(|template| template.name == PRODUCT_VISION)
                .or(list.first())
                .map(|template| template.id.clone());
            if let Some(first) = first {
                template.set(first);
            }
        }
    });
    Effect::new(move |_| {
        let targets = targets.get();
        if let Some(Ok(list)) = all_boards.get()
            && board.get_untracked().is_empty()
            && let Some(first) =
                owner_team.with_value(|team| first_owner_board(&targets, &list, team))
        {
            board.set(first);
        }
    });

    let navigate = use_navigate();
    let create = move |_: ()| {
        if busy.get_untracked() {
            return;
        }
        let document_title = title.get_untracked().trim().to_string();
        if document_title.is_empty() {
            error.set(Some("The title is empty. Write a title.".to_string()));
            return;
        }
        let owner = board.get_untracked();
        if owner.is_empty() {
            error.set(Some(
                "The document has no owner board. Select an owner board.".to_string(),
            ));
            return;
        }
        busy.set(true);
        error.set(None);
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            let body = item_api::CreateDocumentBody {
                title: document_title,
                template_id: template.get_untracked(),
                board: owner,
                parent_short_code: None,
            };
            let created = match item_api::create_document(auth, &body).await {
                Ok(created) => created,
                Err(e) => {
                    busy.set(false);
                    error.set(Some(item_api::error_text(&e)));
                    return;
                }
            };
            let repository = slug.get_value();
            match item_api::add_impact(
                auth,
                item_api::Family::Document,
                &created.short_code,
                &repository,
            )
            .await
            {
                Ok(_) => navigate(
                    &format!("/items/{}", created.short_code),
                    Default::default(),
                ),
                Err(e) => {
                    busy.set(false);
                    error.set(Some(format!(
                        "Kairos made the document {}. Kairos did not make the link to the \
                         repository {repository}. Add the link on the page of the document. \
                         {}",
                        created.short_code,
                        item_api::error_text(&e)
                    )));
                }
            }
        });
    };

    view! {
        <Stack gap="sm" attr:data-testid="repository-document-form">
            {move || error.get().map(|message| view! {
                <Alert title="Kairos did not complete the create" color=token::BAD>
                    <Text size="sm" dimmed=true>{message}</Text>
                </Alert>
            })}
            <TextInput label="Document title" placeholder="e.g. The vision of fidius" value=title/>
            {move || match templates.get() {
                None => view! { <Text size="xs" dimmed=true>"Loading templates…"</Text> }.into_any(),
                Some(Err(error)) => view! {
                    <Text size="sm" dimmed=true>{item_api::error_text(&error)}</Text>
                }.into_any(),
                Some(Ok(list)) if list.is_empty() => view! {
                    <Text size="sm" dimmed=true>
                        "The organization has no templates. An organization admin adds \
                         one on the page Admin, Templates."
                    </Text>
                }.into_any(),
                Some(Ok(list)) => view! {
                    <Select
                        label="Template"
                        option_pairs=list.into_iter().map(|item| (item.id, item.name)).collect()
                        value=template
                        attr:data-testid="repository-document-template"
                    />
                }.into_any(),
            }}
            {move || {
                let targets = targets.get();
                if targets.is_empty() {
                    view! {
                        <Text size="sm" dimmed=true>
                            "You have the capability manage_documents on no board. Ask an \
                             organization admin for it."
                        </Text>
                    }.into_any()
                } else {
                    view! {
                        <Select
                            label="Owner board"
                            option_pairs=targets
                            value=board
                            attr:data-testid="repository-document-board"
                        />
                    }.into_any()
                }
            }}
            <Text size="xs" dimmed=true>
                "The owner board gives the right to edit the document. The document is not a \
                 card on the board. The document impacts the repository: the link says what \
                 the document is about, and it gives no right."
            </Text>
            <Group>
                <Button loading=busy loading_label="Creating…" on_click=Callback::new(create)>
                    "Create document"
                </Button>
            </Group>
        </Stack>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::boards::data::Board;

    fn item(
        entity_type: &str,
        document_type: Option<&str>,
        lifecycle: Option<&str>,
        column: Option<&str>,
    ) -> ImpactingItem {
        ImpactingItem {
            short_code: "ACME-D-0004".into(),
            title: "The vision of fidius".into(),
            entity_type: entity_type.into(),
            document_type: document_type.map(str::to_string),
            lifecycle: lifecycle.map(str::to_string),
            column: column.map(str::to_string),
        }
    }

    #[test]
    fn the_kind_and_the_state_of_an_item() {
        let vision = item("document", Some("vision"), Some("published"), None);
        assert_eq!(kind_label(&vision), "vision");
        assert_eq!(state_label(&vision).as_deref(), Some("published"));
        let plain = item("document", None, Some("draft"), None);
        assert_eq!(kind_label(&plain), "document");
        let adr = item("adr", None, None, Some("Decided"));
        assert_eq!(kind_label(&adr), "ADR");
        assert_eq!(state_label(&adr).as_deref(), Some("Decided"));
        // An ADR that is not on a board has no state.
        assert_eq!(state_label(&item("adr", None, None, None)), None);
    }

    fn board(slug: &str, team: Option<&str>) -> Board {
        Board {
            id: format!("b-{slug}"),
            name: slug.to_uppercase(),
            slug: slug.into(),
            board_level: "delivery".into(),
            team_id: team.map(str::to_string),
        }
    }

    fn target(slug: &str) -> (String, String) {
        (slug.to_string(), slug.to_uppercase())
    }

    /// The board of the owner team of the repository is first, when the
    /// user can give a document to it.
    #[test]
    fn the_first_board_is_the_board_of_the_owner_team() {
        let boards = vec![
            board("initiatives", None),
            board("web-delivery", Some("t-web")),
            board("platform-delivery", Some("t-platform")),
        ];
        let each = vec![
            target("initiatives"),
            target("web-delivery"),
            target("platform-delivery"),
        ];
        assert_eq!(
            first_owner_board(&each, &boards, "t-platform").as_deref(),
            Some("platform-delivery")
        );
        // The user cannot give a document to the board of the owner team.
        let some = vec![target("initiatives"), target("web-delivery")];
        assert_eq!(
            first_owner_board(&some, &boards, "t-platform").as_deref(),
            Some("initiatives")
        );
        assert_eq!(first_owner_board(&[], &boards, "t-platform"), None);
    }

    /// COLLIERY-T-3102: the ADR board of the owner team is not its
    /// delivery board, also when it comes first in the list.
    #[test]
    fn the_adr_board_of_the_owner_team_is_not_first() {
        let mut adrs = board("platform-adrs", Some("t-platform"));
        adrs.board_level = "adr".into();
        let boards = vec![adrs, board("platform-delivery", Some("t-platform"))];
        let each = vec![target("platform-adrs"), target("platform-delivery")];
        assert_eq!(
            first_owner_board(&each, &boards, "t-platform").as_deref(),
            Some("platform-delivery")
        );
    }
}
