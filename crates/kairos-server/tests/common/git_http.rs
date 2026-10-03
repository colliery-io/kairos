//! A git server over HTTP with basic auth, for the tests of the read token
//! of a repository (COLLIERY-T-3105).
//!
//! An axum server on an ephemeral port in front of `git http-backend` (the
//! CGI program that git has for the smart HTTP protocol). Each request must
//! have `Authorization: Basic <user:password>` with the user and the
//! password of the server; else the answer is 401 with
//! `WWW-Authenticate: Basic`, as GitHub gives for a private repository.
//!
//! The server keeps each request: its URI and the user name of its
//! `Authorization` header (when the password was correct). A test uses
//! them to prove that git sent the token in the header and not in the URL.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::Response;
use base64::Engine as _;

/// One request that the server got.
#[derive(Debug, Clone)]
pub struct Seen {
    pub uri: String,
    /// The user name of a correct `Authorization` header. `None`: no
    /// header, or a wrong user or password.
    pub user: Option<String>,
}

#[derive(Clone)]
struct Shared {
    root: PathBuf,
    expected: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

/// A running server.
pub struct GitHttp {
    /// `http://127.0.0.1:{port}`.
    pub base_url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl GitHttp {
    /// Each request so far.
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// Serve the git repositories below `root` at `http://127.0.0.1:{port}/`,
/// for the one `user` with `password`. A repository at `root/x/.git` has
/// the URL `{base_url}/x/.git`.
pub async fn serve(root: PathBuf, user: &str, password: &str) -> GitHttp {
    let expected = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let shared = Shared {
        root,
        expected,
        seen: seen.clone(),
    };
    let router = Router::new().fallback(handle).with_state(shared);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binding an ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("git server");
    });
    GitHttp {
        base_url: format!("http://{addr}"),
        seen,
    }
}

async fn handle(
    State(shared): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let presented = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .map(str::to_string);
    let authorized = presented.as_deref() == Some(shared.expected.as_str());
    let user = authorized.then(|| {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(presented.as_deref().unwrap_or_default())
            .unwrap_or_default();
        String::from_utf8_lossy(&decoded)
            .split(':')
            .next()
            .unwrap_or_default()
            .to_string()
    });
    shared.seen.lock().unwrap().push(Seen {
        uri: uri.to_string(),
        user,
    });
    if !authorized {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("www-authenticate", "Basic realm=\"git\"")
            .body(Body::from("authentication required"))
            .unwrap();
    }
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    let env = vec![
        ("GIT_PROJECT_ROOT", shared.root.display().to_string()),
        ("GIT_HTTP_EXPORT_ALL", "1".to_string()),
        ("PATH_INFO", uri.path().to_string()),
        ("QUERY_STRING", uri.query().unwrap_or_default().to_string()),
        ("REQUEST_METHOD", method.to_string()),
        ("CONTENT_TYPE", header("content-type")),
        ("CONTENT_LENGTH", body.len().to_string()),
        ("HTTP_CONTENT_ENCODING", header("content-encoding")),
        ("GIT_PROTOCOL", header("git-protocol")),
        ("REMOTE_USER", "x".to_string()),
        ("REMOTE_ADDR", "127.0.0.1".to_string()),
    ];
    let output = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut child = std::process::Command::new("git")
            .arg("http-backend")
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("git http-backend starts");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&body)
            .expect("the body goes to git http-backend");
        child.wait_with_output().expect("git http-backend runs")
    })
    .await
    .unwrap();
    cgi_response(&output.stdout)
}

/// The HTTP response of the output of a CGI program.
fn cgi_response(out: &[u8]) -> Response {
    let (head, body) = match find(out, b"\r\n\r\n") {
        Some(at) => (&out[..at], &out[at + 4..]),
        None => match find(out, b"\n\n") {
            Some(at) => (&out[..at], &out[at + 2..]),
            None => (&out[..0], out),
        },
    };
    let mut response = Response::builder().status(StatusCode::OK);
    for line in String::from_utf8_lossy(head).lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("status") {
            let code: u16 = value
                .split_whitespace()
                .next()
                .and_then(|c| c.parse().ok())
                .unwrap_or(500);
            response = response.status(code);
        } else {
            response = response.header(name.trim(), value);
        }
    }
    response.body(Body::from(body.to_vec())).unwrap()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
