//! `GET /api/org/dictionary` (KAIROS-T-0734): the dictionary of the
//! organization. An organization has one dictionary, on the board
//! `strategy`, and no team has a dictionary (KAIROS-A-0079). The read is
//! [`kairos_db::dictionaries::organization_dictionary`].
//!
//! Each member of the organization can read it, as for the items. The web
//! links its terms in the content and the notes of each item, and shows
//! the meaning on hover.

use axum::extract::{Extension, State};
use axum::routing::get;
use axum::{Json, Router};
use kairos_client::types_org::{DictionaryProblem, DictionaryTerm, OrganizationDictionary};

use crate::app::AppState;
use crate::error::ApiError;
use crate::middleware::tenant::TenantContext;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/org/dictionary", get(get_organization_dictionary))
}

/// The dictionary of the organization: its code, its terms and its bad
/// rows. With no dictionary, the code is `null` and the lists are empty.
#[utoipa::path(
    get,
    path = "/api/org/dictionary",
    tag = "organization",
    responses(
        (status = 200, description = "The dictionary of the organization, the terms sorted by the word", body = OrganizationDictionary),
    ),
)]
pub(crate) async fn get_organization_dictionary(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
) -> Result<Json<OrganizationDictionary>, ApiError> {
    let dictionary = state
        .blocking
        .run(&tenant.slug, |conn| {
            kairos_db::dictionaries::organization_dictionary(conn).map_err(ApiError::internal)
        })
        .await?;
    Ok(Json(to_dto(dictionary)))
}

/// The wire shape of the dictionary that the database gives.
fn to_dto(dictionary: kairos_db::dictionaries::OrganizationDictionary) -> OrganizationDictionary {
    OrganizationDictionary {
        short_code: dictionary.short_code,
        terms: dictionary
            .terms
            .into_iter()
            .map(|term| DictionaryTerm {
                word: term.word,
                part_of_speech: term.part_of_speech,
                meaning: term.meaning,
            })
            .collect(),
        problems: dictionary
            .problems
            .into_iter()
            .map(|p| DictionaryProblem {
                line: u32::try_from(p.line).unwrap_or(u32::MAX),
                message: p.message,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dto_keeps_the_code_the_terms_and_the_bad_rows() {
        let content = "| Word | Part of speech | Meaning |\n\
                       |---|---|---|\n\
                       | board | noun | A set of columns. |\n\
                       | lane |\n";
        let read = kairos_db::dictionaries::OrganizationDictionary::from_document(
            Some("X-D-0001".to_string()),
            Some(content),
        );
        let dto = to_dto(read);
        assert_eq!(dto.short_code.as_deref(), Some("X-D-0001"));
        assert_eq!(
            dto.terms,
            vec![DictionaryTerm {
                word: "board".to_string(),
                part_of_speech: "noun".to_string(),
                meaning: "A set of columns.".to_string(),
            }]
        );
        assert_eq!(dto.problems.len(), 1);
        assert_eq!(dto.problems[0].line, 4);
    }

    #[test]
    fn no_dictionary_gives_no_code_and_empty_lists() {
        let dto = to_dto(kairos_db::dictionaries::OrganizationDictionary::default());
        assert_eq!(dto, OrganizationDictionary::default());
    }
}
