//! The wiring of a folder, end to end with the built binary
//! (COLLIERY-I-0660).
//!
//! - KAIROS-T-0706: `kairos mcp-proxy` with no deployment or no credential
//!   serves MCP with no tools, and its instructions say why and what to run.
//!   A `config.toml` that does not parse is refused with the file and the
//!   line, by the proxy and by the CLI verbs.
//!
//! Each test runs the binary in a temp folder with a clean environment: a
//! temp HOME and KAIROS_CONFIG_DIR, no deployment and no key unless the test
//! gives one. No test talks to a deployment: the URLs are RFC 5737
//! addresses that the proxy never calls.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

const KAIROS: &str = env!("CARGO_BIN_EXE_kairos");

/// A temp HOME with a config directory and a work folder.
struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::create_dir_all(dir.path().join("work")).unwrap();
        Sandbox { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn work(&self) -> PathBuf {
        self.path().join("work")
    }

    fn config_file(&self) -> PathBuf {
        self.path().join("config").join("config.toml")
    }

    fn write_config(&self, text: &str) {
        std::fs::write(self.config_file(), text).unwrap();
    }

    /// A credential cache with a login for each of `deployments`.
    fn logins(&self, deployments: &[&str]) {
        let mut all = serde_json::Map::new();
        for deployment in deployments {
            all.insert(
                deployment.to_string(),
                json!({
                    "access_token": "cached",
                    "expires_at": 4_000_000_000u64,
                    "issuer": "",
                    "client_id": "",
                }),
            );
        }
        std::fs::write(
            self.path().join("config").join("credentials.json"),
            json!({"version": 1, "deployments": all}).to_string(),
        )
        .unwrap();
    }

    fn command(&self, args: &[&str], env: &[(&str, &str)]) -> Command {
        let mut command = Command::new(KAIROS);
        command
            .current_dir(self.work())
            .args(args)
            .env_remove("KAIROS_URL")
            .env_remove("KAIROS_KEY")
            .env_remove("KAIROS_MCP_KEY")
            .env_remove("KAIROS_TENANT")
            .env_remove("KAIROS_AUTO_UPDATE")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("CLAUDE_PROJECT_DIR")
            .env("KAIROS_QUIET", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", self.path())
            .env("KAIROS_CONFIG_DIR", self.path().join("config"));
        for (name, value) in env {
            command.env(name, value);
        }
        command
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        self.command(args, env).output().unwrap()
    }

    /// Start `kairos mcp-proxy`, send the handshake and `tools/list`, and
    /// give the answers of `initialize` and `tools/list`, and stderr.
    fn proxy(&self, env: &[(&str, &str)]) -> (Value, Value, String) {
        let mut child = self
            .command(&["mcp-proxy"], env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(value) = serde_json::from_str(&line) {
                    let _ = tx.send(value);
                }
            }
        });
        let send = |stdin: &mut std::process::ChildStdin, message: Value| {
            writeln!(stdin, "{message}").unwrap();
            stdin.flush().unwrap();
        };
        send(
            &mut stdin,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"}
            }}),
        );
        let wait = Duration::from_secs(20);
        let init = rx.recv_timeout(wait).expect("an answer to initialize");
        send(
            &mut stdin,
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        );
        send(
            &mut stdin,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        );
        let tools = rx.recv_timeout(wait).expect("an answer to tools/list");
        drop(stdin);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "the proxy exits 0 at the end");
        (
            init,
            tools,
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The instructions of an answer to `initialize`, and a check that the
/// answer to `tools/list` has no tools.
fn no_tools(init: &Value, tools: &Value) -> String {
    assert_eq!(init["id"], 1, "{init}");
    assert_eq!(tools["result"]["tools"], json!([]), "{tools}");
    let instructions = init["result"]["instructions"]
        .as_str()
        .unwrap_or_else(|| panic!("no instructions: {init}"))
        .to_string();
    assert!(
        instructions.starts_with("The Kairos tools are off"),
        "{instructions}"
    );
    instructions
}

/// KAIROS-T-0706: no login and no URL.
#[test]
fn the_proxy_with_no_login_serves_no_tools_and_says_to_log_in() {
    let sandbox = Sandbox::new();
    let (init, tools, err) = sandbox.proxy(&[]);
    let text = no_tools(&init, &tools);
    assert!(text.contains("no credentials"), "{text}");
    assert!(text.contains("kairos login --url"), "{text}");
    assert!(text.contains("/mcp"), "{text}");
    assert!(err.contains("The Kairos tools are off"), "{err}");
}

/// KAIROS-T-0706: a key with no URL.
#[test]
fn the_proxy_with_a_key_and_no_url_says_to_set_the_url() {
    let sandbox = Sandbox::new();
    let (init, tools, _) = sandbox.proxy(&[("KAIROS_KEY", "stub-key")]);
    let text = no_tools(&init, &tools);
    assert!(
        text.contains("KAIROS_KEY is set, but no URL names the deployment"),
        "{text}"
    );
    assert!(text.contains("Set KAIROS_URL"), "{text}");
    assert!(!text.contains("stub-key"), "the key is not shown: {text}");
}

/// KAIROS-T-0706: logins for two deployments, and no binding.
#[test]
fn the_proxy_with_two_logins_and_no_binding_says_to_bind_the_folder() {
    let sandbox = Sandbox::new();
    sandbox.logins(&["https://192.0.2.10", "https://192.0.2.20"]);
    let (init, tools, _) = sandbox.proxy(&[]);
    let text = no_tools(&init, &tools);
    assert!(text.contains("more than one deployment"), "{text}");
    assert!(
        text.contains("kairos config project --deployment <url>"),
        "{text}"
    );
}

/// KAIROS-T-0706: a `config.toml` that does not parse is not ignored: the
/// proxy names the file and the line, and does not use the login.
#[test]
fn the_proxy_with_a_bad_config_names_the_file_and_the_line() {
    let sandbox = Sandbox::new();
    sandbox.logins(&["https://192.0.2.10"]);
    sandbox.write_config("[default]\ntenant = \"acme\"\ndeploymnet = \"x\"\n");
    let (init, tools, _) = sandbox.proxy(&[]);
    let text = no_tools(&init, &tools);
    assert!(
        text.contains(&sandbox.config_file().display().to_string()),
        "{text}"
    );
    assert!(text.contains("line 3"), "{text}");
    assert!(text.contains("deploymnet"), "{text}");
    assert!(text.contains("kairos config show"), "{text}");
}

/// KAIROS-T-0706: a CLI verb that reads the binding refuses a bad config
/// (exit 1) with the file and the line; it does not use the login.
#[test]
fn a_cli_verb_refuses_a_bad_config() {
    let sandbox = Sandbox::new();
    sandbox.logins(&["https://192.0.2.10"]);
    sandbox
        .write_config("[[project]]\nremote = \"github.com/a/b\"\n\n[[project]]\ntenant = \"x\"\n");
    for args in [&["whoami"][..], &["config", "show"], &["adrs", "check"]] {
        let output = sandbox.run(args, &[]);
        let err = stderr(&output);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {err}");
        assert!(
            err.contains(&sandbox.config_file().display().to_string()) && err.contains("at line 4"),
            "{args:?}: {err}"
        );
        assert!(err.contains("no `remote` and no `path`"), "{args:?}: {err}");
    }
}

/// A bad config with `--json` is the envelope of KAIROS-A-0031.
#[test]
fn a_bad_config_with_json_is_the_error_envelope() {
    let sandbox = Sandbox::new();
    sandbox.write_config("[default\n");
    let output = sandbox.run(&["whoami", "--json"], &[]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let envelope: Value = serde_json::from_str(stderr(&output).trim()).unwrap();
    assert_eq!(envelope["error"]["code"], "CLI_ERROR", "{envelope}");
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(message.contains("line 1"), "{message}");
    assert!(output.stdout.is_empty());
}
