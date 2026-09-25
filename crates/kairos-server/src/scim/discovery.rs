//! RFC 7643 discovery documents: `/scim/v2/ServiceProviderConfig` (§5),
//! `/scim/v2/Schemas` (§7) and `/scim/v2/ResourceTypes` (§6), advertising
//! exactly the subset this server implements (see [`super`] module docs).
//! Served behind the SCIM token like every other `/scim/v2` route — IdPs
//! probe these with the configured credential.

use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Value, json};

use super::error::scim_response;
use super::{GROUP_URN, MAX_COUNT, USER_URN, list_response};

/// `GET /scim/v2/ServiceProviderConfig` (RFC 7643 §5).
pub(crate) async fn service_provider_config() -> Response {
    scim_response(
        StatusCode::OK,
        json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],
            "documentationUri": "https://github.com/colliery-io/kairos",
            "patch": { "supported": true },
            "bulk": { "supported": false, "maxOperations": 0, "maxPayloadSize": 0 },
            "filter": { "supported": true, "maxResults": MAX_COUNT },
            "changePassword": { "supported": false },
            "sort": { "supported": false },
            "etag": { "supported": false },
            "authenticationSchemes": [{
                "name": "OAuth Bearer Token",
                "description": "Per-tenant long-lived bearer token issued by an \
                                org admin via POST /api/scim-tokens (KAIROS-A-0016)",
                "type": "oauthbearertoken",
                "primary": true
            }],
            "meta": {
                "resourceType": "ServiceProviderConfig",
                "location": "/scim/v2/ServiceProviderConfig"
            }
        }),
    )
}

/// The User schema resource: the attribute subset this server round-trips.
fn user_schema() -> Value {
    json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
        "id": USER_URN,
        "name": "User",
        "description": "Kairos user (joined to public.users; membership-scoped per tenant)",
        "attributes": [
            {
                "name": "userName", "type": "string", "multiValued": false,
                "required": true, "caseExact": true, "mutability": "readWrite",
                "returned": "default", "uniqueness": "server",
                // readWrite since KAIROS-T-0184. It was immutable while it was
                // served from `users.external_id`, the OIDC subject — which made
                // every PUT fail for ever at any IdP whose userName is a login
                // email and whose subject is opaque. It has its own column now.
                "description": "The login identifier (users.user_name), commonly \
                                an email. Mutable. Distinct from externalId, \
                                which carries the OIDC subject and is not."
            },
            {
                "name": "displayName", "type": "string", "multiValued": false,
                "required": false, "caseExact": false, "mutability": "readWrite",
                "returned": "default", "uniqueness": "none"
            },
            {
                "name": "active", "type": "boolean", "multiValued": false,
                "required": false, "mutability": "readWrite", "returned": "default",
                "description": "false revokes this tenant's membership immediately"
            },
            {
                "name": "emails", "type": "complex", "multiValued": true,
                "required": false, "mutability": "readWrite", "returned": "default",
                "subAttributes": [
                    { "name": "value", "type": "string", "multiValued": false,
                      "required": true, "mutability": "readWrite", "returned": "default" },
                    { "name": "primary", "type": "boolean", "multiValued": false,
                      "required": false, "mutability": "readWrite", "returned": "default" }
                ]
            }
        ],
        "meta": { "resourceType": "Schema", "location": format!("/scim/v2/Schemas/{USER_URN}") }
    })
}

/// The Group schema resource.
fn group_schema() -> Value {
    json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
        "id": GROUP_URN,
        "name": "Group",
        "description": "kairos-admins (org role mapping) and kairos-team-<slug> (team membership)",
        "attributes": [
            {
                "name": "displayName", "type": "string", "multiValued": false,
                "required": true, "caseExact": true, "mutability": "immutable",
                "returned": "default", "uniqueness": "server"
            },
            {
                "name": "members", "type": "complex", "multiValued": true,
                "required": false, "mutability": "readWrite", "returned": "default",
                "subAttributes": [
                    { "name": "value", "type": "string", "multiValued": false,
                      "required": true, "mutability": "readWrite", "returned": "default",
                      "description": "The member User's id" },
                    { "name": "display", "type": "string", "multiValued": false,
                      "required": false, "mutability": "readOnly", "returned": "default" }
                ]
            }
        ],
        "meta": { "resourceType": "Schema", "location": format!("/scim/v2/Schemas/{GROUP_URN}") }
    })
}

/// `GET /scim/v2/Schemas` (RFC 7643 §7): ListResponse of the two schemas.
pub(crate) async fn schemas() -> Response {
    scim_response(
        StatusCode::OK,
        list_response(2, 1, vec![user_schema(), group_schema()]),
    )
}

/// `GET /scim/v2/ResourceTypes` (RFC 7643 §6).
pub(crate) async fn resource_types() -> Response {
    let resource_type_urn = "urn:ietf:params:scim:schemas:core:2.0:ResourceType";
    scim_response(
        StatusCode::OK,
        list_response(
            2,
            1,
            vec![
                json!({
                    "schemas": [resource_type_urn],
                    "id": "User",
                    "name": "User",
                    "endpoint": "/Users",
                    "schema": USER_URN,
                    "meta": { "resourceType": "ResourceType", "location": "/scim/v2/ResourceTypes/User" }
                }),
                json!({
                    "schemas": [resource_type_urn],
                    "id": "Group",
                    "name": "Group",
                    "endpoint": "/Groups",
                    "schema": GROUP_URN,
                    "meta": { "resourceType": "ResourceType", "location": "/scim/v2/ResourceTypes/Group" }
                }),
            ],
        ),
    )
}
