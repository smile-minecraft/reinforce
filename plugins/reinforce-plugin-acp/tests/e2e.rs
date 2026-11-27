//! End-to-end integration tests for `reinforce-plugin-acp` wired into
//! `reinforce-server` via `--plugins` manifests and driven against a real
//! subprocess ACP fixture agent.

use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use reinforce_core::JobStatus;
use reinforce_server::config::{AuthMode, ServeArgs};
use reinforce_server::state::AppState;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

fn target_tmp_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).expect("create CARGO_TARGET_TMPDIR");
    dir
}

fn plugin_binary() -> &'static str {
    env!("CARGO_BIN_EXE_reinforce-plugin-acp")
}

fn fake_acp_agent() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let dir = target_tmp_dir().join(format!("fake-acp-agent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create fake ACP agent directory");
        let binary = dir.join("fake_acp_agent");
        if binary.exists() {
            return binary;
        }
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_agent.rs");
        let status = ProcessCommand::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .arg("--edition")
            .arg("2021")
            .status()
            .expect("compile fake ACP agent fixture");
        assert!(status.success(), "rustc failed to build fake_acp_agent");
        binary
    })
}

struct Harness {
    app: Router<()>,
    state: AppState,
    dir: TempDir,
}

impl Harness {
    async fn start(plugins: Vec<Value>) -> Self {
        let dir = tempfile::tempdir_in(target_tmp_dir()).expect("create test tempdir");
        let manifest = dir.path().join("plugins.json");
        std::fs::write(&manifest, json!({ "plugins": plugins }).to_string())
            .expect("write plugin manifest");

        let config = ServeArgs {
            host: "127.0.0.1".to_owned(),
            port: 0,
            db: dir.path().join("test.db"),
            blobs: dir.path().join("blobs"),
            fonts: dir.path().join("fonts"),
            auth: AuthMode::Single,
            plugins: Some(manifest),
            demo: false,
        };
        let state = AppState::open(&config).await.expect("open server state");
        Self {
            app: reinforce_server::app(state.clone()),
            state,
            dir,
        }
    }

    async fn call(&self, method: Method, uri: &str, body: Option<Value>) -> Value {
        let (content_type, bytes) = match body {
            Some(value) => (
                Some("application/json"),
                serde_json::to_vec(&value).expect("encode json"),
            ),
            None => (None, Vec::new()),
        };
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "127.0.0.1");
        if let Some(value) = content_type {
            builder = builder.header(header::CONTENT_TYPE, value);
        }
        let request = builder.body(Body::from(bytes)).expect("build request");
        let response = self.app.clone().oneshot(request).await.expect("request");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let payload = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        assert!(
            status == StatusCode::OK || status == StatusCode::ACCEPTED,
            "unexpected HTTP {status} for {uri}: {payload}"
        );
        payload
    }

    async fn submit(&self, plugin: &str, input: Value) -> String {
        let submitted = self
            .call(
                Method::POST,
                &format!("/api/plugins/{plugin}/jobs"),
                Some(json!({
                    "capability": "draft_cards",
                    "input": input,
                })),
            )
            .await;
        assert_eq!(submitted["status"], "queued");
        submitted["id"].as_str().expect("job id").to_owned()
    }

    async fn job(&self, id: &str) -> Value {
        self.call(Method::GET, &format!("/api/plugins/jobs/{id}"), None)
            .await
    }

    async fn wait_status(&self, id: &str, wanted: JobStatus) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let job = self.job(id).await;
            let status = job["status"].as_str().unwrap_or_default();
            if status == wanted.as_str() {
                return job;
            }
            assert!(
                !matches!(status, "succeeded" | "failed" | "cancelled"),
                "job {id} reached terminal {status} while waiting for {}: {job}",
                wanted.as_str()
            );
            assert!(
                Instant::now() < deadline,
                "timed out waiting for job {id} to reach {}: {job}",
                wanted.as_str()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn wait_terminal(&self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let job = self.job(id).await;
            let status = job["status"].as_str().unwrap_or_default();
            if matches!(status, "succeeded" | "failed" | "cancelled") {
                return job;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for job {id} to finish: {job}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn pending_proposal_count(&self) -> usize {
        let list = self
            .call(Method::GET, "/api/proposals?status=pending", None)
            .await;
        list.as_array().map_or(0, Vec::len)
    }

    async fn stored_card_counts(&self) -> (i64, i64) {
        self.state
            .with_db(|database| {
                let kc: i64 = database.connection().query_row(
                    "SELECT COUNT(*) FROM knowledge_cards",
                    [],
                    |row| row.get(0),
                )?;
                let qc: i64 = database.connection().query_row(
                    "SELECT COUNT(*) FROM question_cards",
                    [],
                    |row| row.get(0),
                )?;
                Ok((kc, qc))
            })
            .await
            .expect("count cards")
    }
}

fn acp_manifest_entry(name: &str, extra_plugin_args: &[String], agent_args: &[String]) -> Value {
    let mut args = vec!["--agent".to_owned(), fake_acp_agent().display().to_string()];
    args.extend(extra_plugin_args.iter().cloned());
    if !agent_args.is_empty() {
        args.push("--".to_owned());
        args.extend(agent_args.iter().cloned());
    }
    json!({
        "name": name,
        "capabilities": ["draft_cards"],
        "transport": {
            "type": "stdio",
            "command": plugin_binary(),
            "args": args,
        },
        "timeout_secs": 15,
        "max_output_bytes": 1_048_576,
        "max_attempts": 1,
    })
}

fn process_exists(pid: u32) -> bool {
    ProcessCommand::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

async fn wait_for_pid(path: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                return pid;
            }
        }
        assert!(Instant::now() < deadline, "fake agent never wrote pid file");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn assert_process_gone(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while process_exists(pid) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !process_exists(pid),
        "agent subprocess {pid} is still alive"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manifest_loads_and_produces_pending_unchecked_proposal() {
    let harness = Harness::start(vec![acp_manifest_entry(
        "acp",
        &[],
        &["--mode".to_owned(), "ok".to_owned()],
    )])
    .await;

    let plugins = harness.call(Method::GET, "/api/plugins", None).await;
    assert_eq!(plugins[0]["name"], "acp");
    assert_eq!(plugins[0]["transport"], "stdio");
    assert_eq!(plugins[0]["capabilities"][0], "draft_cards");

    let job_id = harness
        .submit(
            "acp",
            json!({
                "resource_id": "019a0000-0000-7000-8000-000000000001",
                "resource_title": "Newtonian Mechanics",
                "subject": "physics",
                "topics": ["mechanics"],
                "content": "Newton's third law states that every action has an equal and opposite reaction."
            }),
        )
        .await;

    let job = harness.wait_terminal(&job_id).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert_eq!(job["progress"], 1.0);
    let proposal_id = job["proposal_id"]
        .as_str()
        .expect("succeeded job must point to a proposal")
        .to_owned();

    // Proposal is pending and passed the Domain Validator.
    let proposal = harness
        .call(Method::GET, &format!("/api/proposals/{proposal_id}"), None)
        .await;
    assert_eq!(proposal["status"], "pending");
    assert_eq!(proposal["submitted_by"], "agent:plugin:acp");
    let ops = proposal["payload"]["operations"]
        .as_array()
        .expect("proposal operations");
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0]["op"], "knowledge_card");
    assert_eq!(ops[0]["data"]["verification"], "unchecked");
    assert_eq!(ops[0]["data"]["status"], "draft");
    assert_eq!(ops[1]["op"], "question_card");
    assert_eq!(ops[1]["data"]["verification"], "unchecked");
    assert_eq!(ops[1]["data"]["answer_source_kind"], "original_authored");

    // Nothing reaches the live store before owner approval.
    assert_eq!(harness.stored_card_counts().await, (0, 0));

    let approved = harness
        .call(
            Method::POST,
            &format!("/api/proposals/{proposal_id}/approve"),
            Some(json!({"promote_verified": false})),
        )
        .await;
    assert_eq!(approved["status"], "approved");
    assert_eq!(harness.stored_card_counts().await, (1, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn denies_agent_permission_and_fs_requests_while_completing_job() {
    let scratch_dir = tempfile::tempdir_in(target_tmp_dir()).expect("scratch dir");
    let denial_log = scratch_dir.path().join("denials.jsonl");

    let harness = Harness::start(vec![acp_manifest_entry(
        "acp",
        &[],
        &[
            "--mode".to_owned(),
            "permission_and_fs".to_owned(),
            "--scratch".to_owned(),
            denial_log.display().to_string(),
        ],
    )])
    .await;

    let job_id = harness
        .submit(
            "acp",
            json!({
                "subject": "physics",
                "content": "Action and reaction are equal and opposite."
            }),
        )
        .await;

    let job = harness.wait_terminal(&job_id).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert!(job["proposal_id"].is_string());

    let log_text = std::fs::read_to_string(&denial_log).expect("read denial log");
    let replies: Vec<Value> = log_text
        .lines()
        .map(|line| serde_json::from_str(line).expect("json reply"))
        .collect();
    assert_eq!(replies.len(), 4, "expected 4 denial replies: {log_text}");

    // 1. session/request_permission -> selected reject_once option ("deny")
    assert_eq!(replies[0]["id"], 101);
    assert_eq!(replies[0]["result"]["outcome"]["outcome"], "selected");
    assert_eq!(replies[0]["result"]["outcome"]["optionId"], "deny");

    // 2..4. fs/read_text_file, fs/write_text_file, terminal/create -> -32601
    for (idx, expected_id) in [(1, 102), (2, 103), (3, 104)] {
        assert_eq!(replies[idx]["id"], expected_id);
        assert_eq!(replies[idx]["error"]["code"], -32601);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_agent_binary_returns_structured_error_and_fails_job() {
    let missing_agent = target_tmp_dir().join("definitely-does-not-exist-acp-agent");
    let harness = Harness::start(vec![json!({
        "name": "acp-missing",
        "capabilities": ["draft_cards"],
        "transport": {
            "type": "stdio",
            "command": plugin_binary(),
            "args": ["--agent", missing_agent.display().to_string()],
        },
        "timeout_secs": 15,
        "max_output_bytes": 1_048_576,
        "max_attempts": 1,
    })])
    .await;

    let job_id = harness
        .submit(
            "acp-missing",
            json!({"subject": "physics", "content": "Force equals mass times acceleration."}),
        )
        .await;
    let job = harness.wait_terminal(&job_id).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"]["code"], "plugin_reported");
    assert_eq!(job["error"]["details"]["plugin_code"], "agent_not_found");
    assert!(job["proposal_id"].is_null());
    assert_eq!(harness.pending_proposal_count().await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acp_rpc_errors_return_structured_error_and_fail_job() {
    let harness = Harness::start(vec![
        acp_manifest_entry(
            "acp-init-err",
            &[],
            &["--mode".to_owned(), "init_error".to_owned()],
        ),
        acp_manifest_entry(
            "acp-prompt-err",
            &[],
            &["--mode".to_owned(), "prompt_error".to_owned()],
        ),
    ])
    .await;

    for (plugin, expected_msg) in [
        ("acp-init-err", "authentication required"),
        ("acp-prompt-err", "quota exceeded"),
    ] {
        let job_id = harness
            .submit(
                plugin,
                json!({"subject": "physics", "content": "Energy is conserved."}),
            )
            .await;
        let job = harness.wait_terminal(&job_id).await;
        assert_eq!(job["status"], "failed", "{plugin}: {job}");
        assert_eq!(job["error"]["code"], "plugin_reported");
        assert_eq!(job["error"]["details"]["plugin_code"], "acp_error");
        assert!(
            job["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains(expected_msg),
            "{plugin}: {job}"
        );
        assert!(job["proposal_id"].is_null());
    }
    assert_eq!(harness.pending_proposal_count().await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_proposals_and_oversized_output_fail_before_reaching_host() {
    let harness = Harness::start(vec![
        acp_manifest_entry(
            "acp-bad-json",
            &[],
            &["--mode".to_owned(), "invalid_json".to_owned()],
        ),
        acp_manifest_entry(
            "acp-bad-schema",
            &[],
            &["--mode".to_owned(), "invalid_schema".to_owned()],
        ),
        acp_manifest_entry(
            "acp-oversized",
            &[],
            &["--mode".to_owned(), "oversized".to_owned()],
        ),
    ])
    .await;

    for (plugin, expected_code) in [
        ("acp-bad-json", "invalid_proposal"),
        ("acp-bad-schema", "invalid_proposal"),
        ("acp-oversized", "acp_output_too_large"),
    ] {
        let job_id = harness
            .submit(
                plugin,
                json!({"subject": "physics", "content": "Momentum is mass times velocity."}),
            )
            .await;
        let job = harness.wait_terminal(&job_id).await;
        assert_eq!(job["status"], "failed", "{plugin}: {job}");
        assert_eq!(job["error"]["code"], "plugin_reported");
        assert_eq!(job["error"]["details"]["plugin_code"], expected_code);
        assert!(job["proposal_id"].is_null());
    }
    assert_eq!(harness.pending_proposal_count().await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unresponsive_agent_times_out_and_reaps_child() {
    let scratch_dir = tempfile::tempdir_in(target_tmp_dir()).expect("scratch dir");
    let pid_file = scratch_dir.path().join("hang.pid");

    let harness = Harness::start(vec![acp_manifest_entry(
        "acp-timeout",
        &["--request-timeout-secs".to_owned(), "1".to_owned()],
        &[
            "--mode".to_owned(),
            "hang".to_owned(),
            "--pid-file".to_owned(),
            pid_file.display().to_string(),
        ],
    )])
    .await;

    let started = Instant::now();
    let job_id = harness
        .submit(
            "acp-timeout",
            json!({"subject": "physics", "content": "Wait forever."}),
        )
        .await;
    let pid = wait_for_pid(&pid_file).await;

    let job = harness.wait_terminal(&job_id).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"]["code"], "plugin_reported");
    assert_eq!(job["error"]["details"]["plugin_code"], "acp_timeout");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "request timeout must trigger well before server timeout"
    );
    assert_process_gone(pid).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_running_job_sends_session_cancel_and_stops_agent() {
    let scratch_dir = tempfile::tempdir_in(target_tmp_dir()).expect("scratch dir");
    let pid_file = scratch_dir.path().join("cancel.pid");
    let cancel_log = scratch_dir.path().join("cancel.json");

    let harness = Harness::start(vec![acp_manifest_entry(
        "acp-cancel",
        &[],
        &[
            "--mode".to_owned(),
            "wait_cancel".to_owned(),
            "--pid-file".to_owned(),
            pid_file.display().to_string(),
            "--scratch".to_owned(),
            cancel_log.display().to_string(),
        ],
    )])
    .await;

    let job_id = harness
        .submit(
            "acp-cancel",
            json!({"subject": "physics", "content": "Slow task to cancel."}),
        )
        .await;
    harness.wait_status(&job_id, JobStatus::Running).await;
    let pid = wait_for_pid(&pid_file).await;

    let started = Instant::now();
    let _ = harness
        .call(
            Method::POST,
            &format!("/api/plugins/jobs/{job_id}/cancel"),
            None,
        )
        .await;

    let job = harness.wait_terminal(&job_id).await;
    assert_eq!(job["status"], "cancelled", "{job}");
    assert_eq!(job["error"]["code"], "cancelled");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cancel must stop promptly"
    );
    assert_process_gone(pid).await;

    let recorded = std::fs::read_to_string(&cancel_log).expect("agent should receive cancel");
    let cancel_msg: Value = serde_json::from_str(&recorded).expect("parse session/cancel");
    assert_eq!(cancel_msg["method"], "session/cancel");
    assert_eq!(cancel_msg["params"]["sessionId"], "sess_fake_acp_001");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_path_respects_resource_dir_confinement() {
    let resources = tempfile::tempdir_in(target_tmp_dir()).expect("resources dir");
    let outside = tempfile::tempdir_in(target_tmp_dir()).expect("outside dir");
    std::fs::write(
        resources.path().join("chapter.md"),
        "Newton's third law governs interacting force pairs.",
    )
    .expect("write resource");
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, "do not read").expect("write outside");

    let harness = Harness::start(vec![acp_manifest_entry(
        "acp-res",
        &[
            "--resource-dir".to_owned(),
            resources.path().display().to_string(),
        ],
        &["--mode".to_owned(), "ok".to_owned()],
    )])
    .await;
    let _keep_dir = &harness.dir;

    let ok_job = harness
        .submit(
            "acp-res",
            json!({"subject": "physics", "resource_path": "chapter.md"}),
        )
        .await;
    let ok = harness.wait_terminal(&ok_job).await;
    assert_eq!(ok["status"], "succeeded", "{ok}");

    let escaped_job = harness
        .submit(
            "acp-res",
            json!({"subject": "physics", "resource_path": secret.display().to_string()}),
        )
        .await;
    let escaped = harness.wait_terminal(&escaped_job).await;
    assert_eq!(escaped["status"], "failed", "{escaped}");
    assert_eq!(escaped["error"]["code"], "plugin_reported");
    assert_eq!(
        escaped["error"]["details"]["plugin_code"],
        "resource_access_denied"
    );
}

#[test]
fn stdin_eof_after_hello_does_not_spuriously_cancel_job() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = ProcessCommand::new(plugin_binary())
        .args([
            "--agent",
            &fake_acp_agent().display().to_string(),
            "--",
            "--mode",
            "ok",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn reinforce-plugin-acp");

    let hello = json!({
        "type": "hello",
        "protocol": "reinforce_plugin",
        "version": 1,
        "job_id": "019a0000-0000-7000-8000-000000000077",
        "plugin": "acp",
        "capability": "draft_cards",
        "input": {
            "subject": "physics",
            "content": "Newton's third law."
        }
    });
    {
        let mut stdin = child.stdin.take().expect("stdin");
        writeln!(stdin, "{hello}").expect("write hello");
        // Drop stdin immediately (EOF) to mirror shell piping.
    }

    let output = child.wait_with_output().expect("wait for plugin");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let last_line = stdout.lines().last().expect("at least one output line");
    let frame: Value = serde_json::from_str(last_line).expect("parse last frame");
    assert_eq!(frame["type"], "result", "{stdout}");
    assert_eq!(frame["operations"].as_array().map(Vec::len), Some(2));
}
