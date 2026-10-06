//! The session cookie of a password login (KAIROS-T-0327, the 2026-10-06
//! amendment of KAIROS-A-0015).
//!
//! `POST /api/login` returns the session bearer in its body (the CLI uses
//! it) and ALSO sets it as a cookie, so that a reload or a new tab of the
//! GUI keeps the session. The cookie is:
//!
//! - `HttpOnly`: no script of the page can read it, so an XSS cannot take
//!   the credential away;
//! - `Secure`: the browser sends it over HTTPS only (a browser treats
//!   `http://localhost` as secure, so the dev stack works);
//! - `SameSite=Strict`, `Path=/`, `Max-Age` = the session lifetime.
//!
//! `Path=/` and not `/api`: the board view opens `/ws/events`, and the
//! handshake must carry the session too.
//!
//! # A bearer wins
//!
//! [`crate::middleware::auth::require_auth`] reads the cookie only when the
//! request has no `Authorization` header. The CLI, the agents and an API key
//! never meet this path.
//!
//! # The origin check
//!
//! `SameSite=Strict` stops a cross-SITE page, but a page on a sibling
//! host of the same site (`other.tailnet.ts.net` next to
//! `kairos.tailnet.ts.net`) is the same site and gets the cookie. So a
//! request that authenticates with the cookie and that can change state —
//! each method but GET, HEAD and OPTIONS, and each WebSocket handshake
//! (a browser does not apply CORS to it) — must come from the Kairos
//! origin: its `Origin` names the host of the request (or of
//! `KAIROS_PUBLIC_URL`), or, with no `Origin`, `Sec-Fetch-Site` is
//! `same-origin` or `none`. Else the request is refused.

use axum::http::header::{COOKIE, HOST, ORIGIN, UPGRADE};
use axum::http::{HeaderMap, HeaderValue, Method};

/// The name of the cookie.
pub const NAME: &str = "kairos_session";

/// The `Set-Cookie` value of a new session.
pub fn set(token: &str, ttl_secs: u64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{NAME}={token}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={ttl_secs}"
    ))
    .expect("a session token is a valid header value")
}

/// The `Set-Cookie` value that removes the cookie.
pub fn clear() -> HeaderValue {
    HeaderValue::from_static(
        "kairos_session=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0",
    )
}

/// The session token of the cookie, if the request has one.
pub fn token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == NAME)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Does a request with this method and these headers need the origin check?
pub fn needs_origin_check(method: &Method, headers: &HeaderMap) -> bool {
    let safe = matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    let websocket = headers
        .get(UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    !safe || websocket
}

/// The host (and port) of an origin or a URL: `https://a.b:8080/x` → `a.b:8080`.
fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let host = rest.split('/').next()?;
    (!host.is_empty()).then_some(host)
}

/// Does the request come from the Kairos origin? See the module docs.
/// `public_url` is `KAIROS_PUBLIC_URL`; `trusted_proxy` lets
/// `X-Forwarded-Host` name the host too.
pub fn from_kairos_origin(
    headers: &HeaderMap,
    public_url: Option<&str>,
    trusted_proxy: bool,
) -> bool {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    match headers.get(ORIGIN).and_then(|v| v.to_str().ok()) {
        Some(origin) => {
            let Some(origin_host) = host_of(origin) else {
                return false; // "null", or not a URL
            };
            let mut hosts: Vec<&str> = Vec::new();
            if let Some(host) = headers.get(HOST).and_then(|v| v.to_str().ok()) {
                hosts.push(host);
            }
            if trusted_proxy && let Some(forwarded) = text("x-forwarded-host") {
                hosts.push(forwarded);
            }
            if let Some(public) = public_url.and_then(host_of) {
                hosts.push(public);
            }
            hosts
                .iter()
                .any(|host| host.eq_ignore_ascii_case(origin_host))
        }
        None => matches!(text("sec-fetch-site"), Some("same-origin" | "none")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    #[test]
    fn the_cookie_has_each_attribute() {
        let value = set("kairos_ss_abc", 60);
        assert_eq!(
            value.to_str().unwrap(),
            "kairos_session=kairos_ss_abc; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=60"
        );
        assert!(clear().to_str().unwrap().contains("Max-Age=0"));
    }

    #[test]
    fn the_token_is_read_from_the_cookie_header() {
        assert_eq!(
            token(&headers(&[(
                "cookie",
                "a=1; kairos_session=kairos_ss_x; b=2"
            )])),
            Some("kairos_ss_x".to_string())
        );
        assert_eq!(
            token(&headers(&[
                ("cookie", "a=1"),
                ("cookie", "kairos_session=t")
            ])),
            Some("t".to_string())
        );
        assert_eq!(token(&headers(&[("cookie", "kairos_session=")])), None);
        assert_eq!(token(&headers(&[("cookie", "xkairos_session=t")])), None);
        assert_eq!(token(&headers(&[])), None);
    }

    #[test]
    fn a_change_and_a_websocket_need_the_origin_check() {
        let none = headers(&[]);
        assert!(!needs_origin_check(&Method::GET, &none));
        assert!(!needs_origin_check(&Method::HEAD, &none));
        assert!(needs_origin_check(&Method::POST, &none));
        assert!(needs_origin_check(&Method::PATCH, &none));
        assert!(needs_origin_check(&Method::DELETE, &none));
        assert!(needs_origin_check(
            &Method::GET,
            &headers(&[("upgrade", "websocket")])
        ));
    }

    #[test]
    fn only_the_kairos_origin_passes() {
        let cfg = Some("https://kairos.tail.ts.net");
        let check = |h: &HeaderMap, public: Option<&str>| from_kairos_origin(h, public, false);
        let host = ("host", "kairos.tail.ts.net");
        assert!(check(
            &headers(&[host, ("origin", "https://kairos.tail.ts.net")]),
            cfg
        ));
        // A sibling host of the same site is refused.
        assert!(!check(
            &headers(&[host, ("origin", "https://skadi.tail.ts.net")]),
            cfg
        ));
        assert!(!check(&headers(&[host, ("origin", "null")]), cfg));
        // The public URL counts when the Host is an internal name.
        assert!(check(
            &headers(&[
                ("host", "kairos:8080"),
                ("origin", "https://kairos.tail.ts.net")
            ]),
            cfg
        ));
        // The dev stack: no public URL, the Host is the origin.
        assert!(check(
            &headers(&[
                ("host", "localhost:41080"),
                ("origin", "http://localhost:41080")
            ]),
            None
        ));
        // A trusted proxy may name the host in X-Forwarded-Host.
        let proxied = headers(&[
            ("host", "kairos:8080"),
            ("x-forwarded-host", "k.example"),
            ("origin", "https://k.example"),
        ]);
        assert!(from_kairos_origin(&proxied, None, true));
        assert!(!from_kairos_origin(&proxied, None, false));
        // No Origin: Sec-Fetch-Site decides.
        assert!(check(
            &headers(&[host, ("sec-fetch-site", "same-origin")]),
            cfg
        ));
        assert!(!check(
            &headers(&[host, ("sec-fetch-site", "same-site")]),
            cfg
        ));
        assert!(!check(&headers(&[host]), cfg));
    }
}
