//! End-to-end tests for the external plugin queue.
//!
//! Every test runs a real child process: `tests/fixtures/fake_plugin.rs` is
//! compiled once per test binary with `rustc`, so the queue, the stdio runner,
//! the timeout/cancel kills and the proposal hand-off are exercised against a
//! genuine process rather than a mock.

use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use reinforce_core::JobStatus;
use reinforce_core::auth::set_setting;
use serde_json::{Value, json};
use tower::ServiceExt;

use reinforce_server::auth::{create_agent_token, hash_token};
use reinforce_server::config::{AuthMode, ServeArgs};
use reinforce_server::state::AppState;

const OWNER_SETUP_TOKEN: &str = "plugin-test-owner-token";
const OWNER_PASSWORD: &str = "plugin-test-owner-password";
const MEMBER_PASSWORD: &str = "plugin-test-member-password";

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn fake_plugin() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let directory =
            std::env::temp_dir().join(format!("reinforce-fake-plugin-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create fake plugin directory");
        let binary = directory.join("fake_plugin");
        if binary.exists() {
            return binary;
        }
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_plugin.rs");
        let status = ProcessCommand::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .arg("--edition")
            .arg("2021")
            .status()
            .expect("run rustc for the fake plugin");
        assert!(status.success(), "rustc could not build the fake plugin");
        binary
    })
}

struct TestDir(PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    app: Router<()>,
    state: AppState,
    /// Keeps the temporary directory (and its database) for the test's lifetime.
    _directory: TestDir,
}

impl Fixture {
    async fn submit(&self, plugin: &str, body: Value) -> axum::response::Response {
        json_call(
            &self.app,
            Method::POST,
            &format!("/api/plugins/{plugin}/jobs"),
            Some(body),
            None,
        )
        .await
    }

    async fn submit_ok(&self, plugin: &str) -> Value {
        let response = self
            .submit(
                plugin,
                json!({"capability": "ingest_pdf", "input": {"resource_id": "r1"}}),
            )
            .await;
        expect_json(response, StatusCode::ACCEPTED).await
    }

    async fn job(&self, id: &str) -> Value {
        expect_json(
            json_call(
                &self.app,
                Method::GET,
                &format!("/api/plugins/jobs/{id}"),
                None,
                None,
            )
            .await,
            StatusCode::OK,
        )
        .await
    }

    async fn wait_status(&self, id: &str, wanted: JobStatus) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let job = self.job(id).await;
            let status = job["status"].as_str().unwrap_or_default();
            if status == wanted.as_str() {
                return job;
            }
            assert!(
                !matches!(status, "succeeded" | "failed" | "cancelled"),
                "job {id} reached {status} while waiting for {}: {job}",
                wanted.as_str()
            );
            assert!(
                Instant::now() < deadline,
                "job {id} stayed {status} while waiting for {}: {job}",
                wanted.as_str()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn wait_terminal(&self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let job = self.job(id).await;
            let status = job["status"].as_str().unwrap_or_default();
            if matches!(status, "succeeded" | "failed" | "cancelled") {
                return job;
            }
            assert!(Instant::now() < deadline, "job {id} stayed {status}: {job}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn proposal(&self, id: &str) -> Value {
        expect_json(
            json_call(
                &self.app,
                Method::GET,
                &format!("/api/proposals/{id}"),
                None,
                None,
            )
            .await,
            StatusCode::OK,
        )
        .await
    }

    async fn published_card_count(&self) -> i64 {
        self.state
            .with_db(|database| {
                let count: i64 = database.connection().query_row(
                    "SELECT COUNT(*) FROM knowledge_cards",
                    [],
                    |row| row.get(0),
                )?;
                Ok(count)
            })
            .await
            .expect("count knowledge cards")
    }
}

struct Setup {
    directory: PathBuf,
    mode: AuthMode,
    plugins: Vec<Value>,
}

impl Setup {
    fn new(mode: AuthMode) -> Self {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "reinforce-plugin-test-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        Self {
            directory,
            mode,
            plugins: Vec::new(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    fn plugin(&mut self, name: &str, mode: &str, args: &[String]) -> &mut Self {
        self.plugin_with(name, mode, args, 5, 65_536, 1)
    }

    fn plugin_with(
        &mut self,
        name: &str,
        mode: &str,
        args: &[String],
        timeout_secs: u64,
        max_output_bytes: usize,
        max_attempts: u32,
    ) -> &mut Self {
        let mut arguments = vec![mode.to_owned()];
        arguments.extend(args.iter().cloned());
        self.plugins.push(json!({
            "name": name,
            "capabilities": ["ingest_pdf"],
            "transport": {
                "type": "stdio",
                "command": fake_plugin().display().to_string(),
                "args": arguments,
            },
            "timeout_secs": timeout_secs,
            "max_output_bytes": max_output_bytes,
            "max_attempts": max_attempts,
        }));
        self
    }

    fn manifest(&self) -> PathBuf {
        self.directory.join("plugins.json")
    }

    async fn open(self) -> Fixture {
        let manifest = self.manifest();
        std::fs::write(&manifest, json!({ "plugins": self.plugins }).to_string())
            .expect("write plugin manifest");
        let config = ServeArgs {
            host: "127.0.0.1".to_owned(),
            port: 0,
            db: self.directory.join("test.db"),
            blobs: self.directory.join("blobs"),
            fonts: self.directory.join("fonts"),
            auth: self.mode,
            plugins: Some(manifest),
            demo: false,
        };
        let state = AppState::open(&config).await.expect("open test state");
        Fixture {
            app: reinforce_server::app(state.clone()),
            state,
            _directory: TestDir(self.directory),
        }
    }
}

async fn call(
    app: &Router<()>,
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: Option<String>,
    bearer: Option<String>,
) -> axum::response::Response {
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
    if let Some(value) = cookie {
        builder = builder.header(header::COOKIE, value);
    }
    if let Some(value) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {value}"));
    }
    let request = builder.body(Body::from(bytes)).expect("build request");
    app.clone().oneshot(request).await.expect("run request")
}

async fn json_call(
    app: &Router<()>,
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: Option<String>,
) -> axum::response::Response {
    call(app, method, uri, body, cookie, None).await
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    if bytes.is_empty() {
        return Value::Null;
    }
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

async fn expect_json(response: axum::response::Response, status: StatusCode) -> Value {
    let actual = response.status();
    let body = response_json(response).await;
    assert_eq!(actual, status, "{body}");
    body
}

fn session_cookie(response: &axum::response::Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie header")
        .to_str()
        .expect("cookie text")
        .split(';')
        .next()
        .expect("cookie pair")
        .to_owned()
}

async fn setup_owner(fixture: &Fixture) -> String {
    fixture
        .state
        .with_db(|database| {
            set_setting(
                database.connection(),
                "owner_setup_token_hash",
                &hash_token(OWNER_SETUP_TOKEN),
            )?;
            Ok(())
        })
        .await
        .expect("set owner setup token");
    let response = json_call(
        &fixture.app,
        Method::POST,
        "/api/auth/owner/setup",
        Some(json!({"token": OWNER_SETUP_TOKEN, "username": "owner", "password": OWNER_PASSWORD})),
        None,
    )
    .await;
    expect_json(response, StatusCode::OK).await;
    let response = json_call(
        &fixture.app,
        Method::POST,
        "/api/auth/login",
        Some(json!({"username": "owner", "password": OWNER_PASSWORD})),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    session_cookie(&response)
}

async fn register_member(fixture: &Fixture, owner_cookie: &str) -> String {
    let response = json_call(
        &fixture.app,
        Method::POST,
        "/api/auth/invites",
        Some(json!({"expires_in_secs": 3600})),
        Some(owner_cookie.to_owned()),
    )
    .await;
    let invite = expect_json(response, StatusCode::OK).await;
    let code = invite["code"].as_str().expect("invite code").to_owned();
    let response = json_call(
        &fixture.app,
        Method::POST,
        "/api/auth/register",
        Some(json!({
            "invite_code": code,
            "username": "learner",
            "password": MEMBER_PASSWORD,
        })),
        None,
    )
    .await;
    expect_json(response, StatusCode::OK).await;
    let response = json_call(
        &fixture.app,
        Method::POST,
        "/api/auth/login",
        Some(json!({"username": "learner", "password": MEMBER_PASSWORD})),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    session_cookie(&response)
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

async fn plugin_pid(path: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                return pid;
            }
        }
        assert!(Instant::now() < deadline, "plugin never wrote its pid file");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn assert_process_gone(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while process_exists(pid) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!process_exists(pid), "plugin process {pid} is still alive");
}

async fn server_is_healthy(fixture: &Fixture) {
    let response = json_call(&fixture.app, Method::GET, "/api/meta", None, None).await;
    let meta = expect_json(response, StatusCode::OK).await;
    assert_eq!(meta["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_plugin_turns_work_into_a_pending_unchecked_proposal() {
    let mut setup = Setup::new(AuthMode::Single);
    setup.plugin("fake", "ok", &[]);
    let fixture = setup.open().await;

    let plugins = expect_json(
        json_call(&fixture.app, Method::GET, "/api/plugins", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(plugins[0]["name"], "fake");
    assert_eq!(plugins[0]["transport"], "stdio");
    assert_eq!(plugins[0]["capabilities"][0], "ingest_pdf");
    assert_eq!(plugins[0]["timeout_secs"], 5);

    let submitted = fixture.submit_ok("fake").await;
    assert_eq!(submitted["status"], "queued");
    assert_eq!(submitted["attempts"], 0);
    let job_id = submitted["id"].as_str().expect("job id").to_owned();

    let job = fixture.wait_terminal(&job_id).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert_eq!(job["attempts"], 1);
    assert_eq!(job["progress"], 1.0);
    let proposal_id = job["proposal_id"]
        .as_str()
        .expect("succeeded jobs point at their proposal")
        .to_owned();

    let proposal = fixture.proposal(&proposal_id).await;
    assert_eq!(proposal["status"], "pending");
    assert_eq!(proposal["submitted_by"], "agent:plugin:fake");
    let operation = &proposal["payload"]["operations"][0];
    assert_eq!(operation["op"], "knowledge_card");
    // The plugin claimed "verified"; the pipeline forces unchecked.
    assert_eq!(operation["data"]["verification"], "unchecked");
    assert_eq!(operation["data"]["status"], "published");
    let card_id = operation["data"]["id"]
        .as_str()
        .expect("resolved card id")
        .to_owned();

    // Nothing exists in the content store until an owner approves.
    assert_eq!(fixture.published_card_count().await, 0);
    let missing = json_call(
        &fixture.app,
        Method::GET,
        &format!("/api/knowledge_cards/{card_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // The owner review is the only way in.
    let approved = expect_json(
        json_call(
            &fixture.app,
            Method::POST,
            &format!("/api/proposals/{proposal_id}/approve"),
            Some(json!({"promote_verified": false})),
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(approved["status"], "approved");
    let card = expect_json(
        json_call(
            &fixture.app,
            Method::GET,
            &format!("/api/knowledge_cards/{card_id}"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(card["verification"], "unchecked");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hung_plugin_times_out_and_is_killed() {
    let mut setup = Setup::new(AuthMode::Single);
    let pid_file = setup.path("hang.pid");
    setup.plugin_with(
        "hang",
        "hang",
        &[pid_file.display().to_string()],
        1,
        65_536,
        1,
    );
    let fixture = setup.open().await;

    let submitted = fixture.submit_ok("hang").await;
    let job_id = submitted["id"].as_str().expect("job id").to_owned();
    let pid = plugin_pid(&pid_file).await;

    let job = fixture.wait_terminal(&job_id).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"]["code"], "timeout");
    assert!(job["error"]["message"].as_str().unwrap().contains("1s"));
    assert_process_gone(pid).await;
    server_is_healthy(&fixture).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_crashing_plugin_fails_the_job_with_its_exit_status() {
    let mut setup = Setup::new(AuthMode::Single);
    setup.plugin("crash", "crash", &[]);
    setup.plugin("crash_before_ready", "crash_before_ready", &[]);
    let fixture = setup.open().await;

    let first = fixture.submit_ok("crash").await;
    let job = fixture
        .wait_terminal(first["id"].as_str().expect("job id"))
        .await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"]["code"], "crashed");
    assert!(
        job["error"]["message"]
            .as_str()
            .unwrap()
            .contains("exit status")
    );
    assert!(
        job["error"]["details"]["stderr"]
            .as_str()
            .unwrap_or_default()
            .contains("crashed on purpose")
    );

    let second = fixture.submit_ok("crash_before_ready").await;
    let job = fixture
        .wait_terminal(second["id"].as_str().expect("job id"))
        .await;
    assert_eq!(job["status"], "failed");
    assert_eq!(job["error"]["code"], "crashed");
    server_is_healthy(&fixture).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_output_is_cut_off() {
    let mut setup = Setup::new(AuthMode::Single);
    setup.plugin_with("noisy", "overflow", &[], 5, 1_024, 1);
    let fixture = setup.open().await;

    let submitted = fixture.submit_ok("noisy").await;
    let job = fixture
        .wait_terminal(submitted["id"].as_str().expect("job id"))
        .await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"]["code"], "output_too_large");
    assert!(job["error"]["message"].as_str().unwrap().contains("1024"));
    server_is_healthy(&fixture).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn output_without_newlines_is_cut_off_at_the_cap() {
    let mut setup = Setup::new(AuthMode::Single);
    setup.plugin_with("flood", "flood", &[], 30, 1_024, 1);
    let fixture = setup.open().await;

    let started = Instant::now();
    let submitted = fixture.submit_ok("flood").await;
    let job = fixture
        .wait_terminal(submitted["id"].as_str().expect("job id"))
        .await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"]["code"], "output_too_large", "{job}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the cap must trip long before the 30s timeout"
    );
    server_is_healthy(&fixture).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_jobs_for_a_removed_capability_do_not_run() {
    let mut setup = Setup::new(AuthMode::Single);
    setup.plugin("fake", "ok", &[]);
    let stale_id = {
        let database =
            reinforce_core::Database::open(setup.path("test.db")).expect("open database");
        let stale = reinforce_core::PluginJob::queued(
            "fake",
            "removed_capability",
            json!({"resource_id": "r1"}),
            1,
        );
        reinforce_core::plugin_store::insert_job(database.connection(), &stale).expect("insert");
        stale.id
    };
    let fixture = setup.open().await;

    let job = fixture.wait_terminal(&stale_id.to_string()).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"]["code"], "plugin_not_configured", "{job}");
    assert!(job["proposal_id"].is_null(), "{job}");
    assert_eq!(fixture.published_card_count().await, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_violations_fail_the_job_cleanly() {
    let mut setup = Setup::new(AuthMode::Single);
    setup.plugin("old_version", "bad_version", &[]);
    setup.plugin("garbage", "bad_json", &[]);
    setup.plugin("wrong_job", "wrong_job_id", &[]);
    let fixture = setup.open().await;

    for plugin in ["old_version", "garbage", "wrong_job"] {
        let submitted = fixture.submit_ok(plugin).await;
        let job = fixture
            .wait_terminal(submitted["id"].as_str().expect("job id"))
            .await;
        assert_eq!(job["status"], "failed", "{plugin}: {job}");
        assert_eq!(job["error"]["code"], "protocol_violation", "{plugin}");
    }
    server_is_healthy(&fixture).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_running_job_stops_the_process() {
    let mut setup = Setup::new(AuthMode::Single);
    let pid_file = setup.path("slow.pid");
    setup.plugin("slow", "slow", &[pid_file.display().to_string()]);
    let fixture = setup.open().await;

    let submitted = fixture.submit_ok("slow").await;
    let job_id = submitted["id"].as_str().expect("job id").to_owned();
    let running = fixture.wait_status(&job_id, JobStatus::Running).await;
    assert_eq!(running["status"], "running");
    let pid = plugin_pid(&pid_file).await;

    let cancelled = expect_json(
        json_call(
            &fixture.app,
            Method::POST,
            &format!("/api/plugins/jobs/{job_id}/cancel"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(cancelled["status"], "running");
    assert_eq!(cancelled["cancel_requested"], true);

    let job = fixture.wait_terminal(&job_id).await;
    assert_eq!(job["status"], "cancelled", "{job}");
    assert_eq!(job["error"]["code"], "cancelled");
    assert_process_gone(pid).await;
    server_is_healthy(&fixture).await;
}

/// Pads the job input so a handshake the plugin never reads leaves less room
/// in a 64 KiB pipe than the cancel message needs.
fn input_that_fills_the_pipe(plugin: &str) -> Value {
    use reinforce_plugin_protocol::{Hello, HostMessage};
    const PIPE_CAPACITY: usize = 64 * 1024;
    let unread = |pad: usize| {
        let input = json!({"pad": "x".repeat(pad)});
        let hello = HostMessage::Hello(Hello::new(
            "00000000-0000-0000-0000-000000000000",
            plugin,
            "ingest_pdf",
            input,
        ));
        serde_json::to_string(&hello).expect("encode hello").len() + 1
    };
    let target = PIPE_CAPACITY - 16;
    let pad = target - unread(0);
    assert_eq!(unread(pad), target);
    json!({"pad": "x".repeat(pad)})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_plugin_that_stopped_reading_stdin_is_prompt() {
    let mut setup = Setup::new(AuthMode::Single);
    setup.plugin_with("stall", "never_read", &[], 30, 65_536, 1);
    let fixture = setup.open().await;

    let response = fixture
        .submit(
            "stall",
            json!({"capability": "ingest_pdf", "input": input_that_fills_the_pipe("stall")}),
        )
        .await;
    let submitted = expect_json(response, StatusCode::ACCEPTED).await;
    let id = submitted["id"].as_str().expect("job id").to_owned();
    fixture.wait_status(&id, JobStatus::Running).await;
    // Let the host finish writing the handshake and start waiting for ready.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let started = Instant::now();
    let response = json_call(
        &fixture.app,
        Method::POST,
        &format!("/api/plugins/jobs/{id}/cancel"),
        None,
        None,
    )
    .await;
    assert!(response.status().is_success(), "{}", response.status());
    let job = fixture.wait_terminal(&id).await;
    assert_eq!(job["status"], "cancelled", "{job}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "cancel must not wait for the 30s timeout"
    );
    server_is_healthy(&fixture).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_queued_job_never_starts_it() {
    let mut setup = Setup::new(AuthMode::Single);
    let pid_file = setup.path("busy.pid");
    setup.plugin("busy", "slow", &[pid_file.display().to_string()]);
    let fixture = setup.open().await;

    let first = fixture.submit_ok("busy").await;
    let first_id = first["id"].as_str().expect("job id").to_owned();
    fixture.wait_status(&first_id, JobStatus::Running).await;
    let first_pid = plugin_pid(&pid_file).await;

    let second = fixture.submit_ok("busy").await;
    let second_id = second["id"].as_str().expect("job id").to_owned();
    assert_eq!(second["status"], "queued");
    let cancelled = expect_json(
        json_call(
            &fixture.app,
            Method::POST,
            &format!("/api/plugins/jobs/{second_id}/cancel"),
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(cancelled["attempts"], 0);

    // Finish the running job so the test leaves no child behind.
    json_call(
        &fixture.app,
        Method::POST,
        &format!("/api/plugins/jobs/{first_id}/cancel"),
        None,
        None,
    )
    .await;
    let job = fixture.wait_terminal(&first_id).await;
    assert_eq!(job["status"], "cancelled");
    assert_process_gone(first_pid).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_errors_and_invalid_batches_fail_without_writing_content() {
    let mut setup = Setup::new(AuthMode::Single);
    setup.plugin("angry", "error", &[]);
    setup.plugin("invalid", "invalid_batch", &[]);
    let fixture = setup.open().await;

    let submitted = fixture.submit_ok("angry").await;
    let job = fixture
        .wait_terminal(submitted["id"].as_str().expect("job id"))
        .await;
    assert_eq!(job["status"], "failed");
    assert_eq!(job["error"]["code"], "plugin_reported");
    assert_eq!(job["error"]["details"]["plugin_code"], "unsupported_page");

    let submitted = fixture.submit_ok("invalid").await;
    let job = fixture
        .wait_terminal(submitted["id"].as_str().expect("job id"))
        .await;
    assert_eq!(job["status"], "failed");
    assert_eq!(job["error"]["code"], "validation_failed");
    let errors = job["error"]["details"]["errors"]
        .as_array()
        .expect("field-level validation errors");
    assert!(!errors.is_empty());
    assert_eq!(errors[0]["rule"], "referential_integrity");
    assert!(job["proposal_id"].is_null());
    assert_eq!(fixture.published_card_count().await, 0);

    let pending = expect_json(
        json_call(
            &fixture.app,
            Method::GET,
            "/api/proposals?status=pending",
            None,
            None,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(pending.as_array().map(Vec::len), Some(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transient_failures_retry_up_to_the_limit() {
    let mut setup = Setup::new(AuthMode::Single);
    let scratch = setup.path("retry-state");
    setup.plugin_with(
        "retry",
        "retry_once",
        &[scratch.display().to_string()],
        5,
        65_536,
        2,
    );
    let fixture = setup.open().await;

    let submitted = fixture.submit_ok("retry").await;
    let job = fixture
        .wait_terminal(submitted["id"].as_str().expect("job id"))
        .await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert_eq!(job["attempts"], 2);
    assert!(job["proposal_id"].is_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_fails_running_jobs_and_resumes_queued_ones() {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "reinforce-plugin-restart-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create test directory");
    let directory = TestDir(directory);
    let manifest = directory.0.join("plugins.json");
    std::fs::write(
        &manifest,
        json!({"plugins": [{
            "name": "fake",
            "capabilities": ["ingest_pdf"],
            "transport": {
                "type": "stdio",
                "command": fake_plugin().display().to_string(),
                "args": ["ok"],
            },
        }]})
        .to_string(),
    )
    .expect("write manifest");
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: directory.0.join("test.db"),
        blobs: directory.0.join("blobs"),
        fonts: directory.0.join("fonts"),
        auth: AuthMode::Single,
        plugins: Some(manifest),
        demo: false,
    };

    // A previous server process left one running job and one queued job.
    let running_id = {
        let mut database =
            reinforce_core::Database::open(&config.db).expect("open database directly");
        let running = reinforce_core::PluginJob::queued(
            "fake",
            "ingest_pdf",
            json!({"resource_id": "running"}),
            1,
        );
        let queued = reinforce_core::PluginJob::queued(
            "fake",
            "ingest_pdf",
            json!({"resource_id": "queued"}),
            1,
        );
        reinforce_core::plugin_store::insert_job(database.connection(), &running).expect("insert");
        reinforce_core::plugin_store::insert_job(database.connection(), &queued).expect("insert");
        let claimed = reinforce_core::plugin_store::claim_next_queued(database.connection_mut())
            .expect("claim")
            .expect("a queued job");
        assert_eq!(claimed.id, running.id);
        running.id
    };

    let state = AppState::open(&config).await.expect("reopen state");
    let app = reinforce_server::app(state.clone());
    let fixture = Fixture {
        app,
        state,
        _directory: directory,
    };

    let interrupted = fixture.job(&running_id.to_string()).await;
    assert_eq!(interrupted["status"], "failed", "{interrupted}");
    assert_eq!(interrupted["error"]["code"], "interrupted");

    // The queued job survived the restart and runs now.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut succeeded = false;
    while Instant::now() < deadline {
        let jobs = expect_json(
            json_call(
                &fixture.app,
                Method::GET,
                "/api/plugins/jobs?status=succeeded",
                None,
                None,
            )
            .await,
            StatusCode::OK,
        )
        .await;
        if jobs.as_array().is_some_and(|jobs| jobs.len() == 1) {
            succeeded = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(succeeded, "the queued job should resume after a restart");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_a_plugin_fails_its_queued_jobs() {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "reinforce-plugin-orphan-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create test directory");
    let directory = TestDir(directory);
    let manifest = directory.0.join("plugins.json");
    std::fs::write(&manifest, json!({"plugins": []}).to_string()).expect("write manifest");
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: directory.0.join("test.db"),
        blobs: directory.0.join("blobs"),
        fonts: directory.0.join("fonts"),
        auth: AuthMode::Single,
        plugins: Some(manifest),
        demo: false,
    };

    let orphan_id = {
        let database = reinforce_core::Database::open(&config.db).expect("open database");
        let orphan = reinforce_core::PluginJob::queued(
            "removed-plugin",
            "ingest_pdf",
            json!({"resource_id": "r1"}),
            1,
        );
        reinforce_core::plugin_store::insert_job(database.connection(), &orphan).expect("insert");
        orphan.id
    };

    let state = AppState::open(&config).await.expect("reopen state");
    let app = reinforce_server::app(state.clone());
    let fixture = Fixture {
        app,
        state,
        _directory: directory,
    };
    let job = fixture.job(&orphan_id.to_string()).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"]["code"], "plugin_not_configured");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_owners_and_agent_tokens_cannot_queue_work() {
    let mut setup = Setup::new(AuthMode::Invite);
    setup.plugin("fake", "ok", &[]);
    let fixture = setup.open().await;
    let owner_cookie = setup_owner(&fixture).await;
    let member_cookie = register_member(&fixture, &owner_cookie).await;
    let agent_token = fixture
        .state
        .with_db(|database| create_agent_token(database.connection()))
        .await
        .expect("create agent token");

    let body = json!({"capability": "ingest_pdf", "input": {"resource_id": "r1"}});

    let member = json_call(
        &fixture.app,
        Method::POST,
        "/api/plugins/fake/jobs",
        Some(body.clone()),
        Some(member_cookie.clone()),
    )
    .await;
    assert_eq!(member.status(), StatusCode::FORBIDDEN);

    let member_list = json_call(
        &fixture.app,
        Method::GET,
        "/api/plugins/jobs",
        None,
        Some(member_cookie),
    )
    .await;
    assert_eq!(member_list.status(), StatusCode::FORBIDDEN);

    // An agent token is not a session: the REST API rejects it outright.
    for method in [Method::POST, Method::GET] {
        let uri = if method == Method::POST {
            "/api/plugins/fake/jobs"
        } else {
            "/api/plugins/jobs"
        };
        let body = (method == Method::POST).then(|| body.clone());
        let response = call(
            &fixture.app,
            method,
            uri,
            body,
            None,
            Some(agent_token.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    let owner = json_call(
        &fixture.app,
        Method::POST,
        "/api/plugins/fake/jobs",
        Some(body),
        Some(owner_cookie),
    )
    .await;
    assert_eq!(owner.status(), StatusCode::ACCEPTED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_api_cannot_create_or_reconfigure_plugins() {
    let mut setup = Setup::new(AuthMode::Single);
    setup.plugin("fake", "ok", &[]);
    let fixture = setup.open().await;

    // No route creates a plugin.
    let created = json_call(
        &fixture.app,
        Method::POST,
        "/api/plugins",
        Some(json!({"name": "evil", "transport": {"type": "stdio", "command": "/bin/sh"}})),
        None,
    )
    .await;
    assert_eq!(created.status(), StatusCode::METHOD_NOT_ALLOWED);

    for method in [Method::PUT, Method::PATCH, Method::DELETE] {
        let response = json_call(
            &fixture.app,
            method,
            "/api/plugins/fake",
            Some(json!({"command": "/bin/sh"})),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // A submission cannot smuggle a command, arguments or a path.
    let smuggled = fixture
        .submit(
            "fake",
            json!({
                "capability": "ingest_pdf",
                "input": {"resource_id": "r1"},
                "command": "/bin/sh",
            }),
        )
        .await;
    assert_eq!(smuggled.status(), StatusCode::BAD_REQUEST);

    let unknown = fixture
        .submit(
            "does-not-exist",
            json!({"capability": "ingest_pdf", "input": {"resource_id": "r1"}}),
        )
        .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let wrong_capability = fixture
        .submit(
            "fake",
            json!({"capability": "tikz_render", "input": {"resource_id": "r1"}}),
        )
        .await;
    assert_eq!(wrong_capability.status(), StatusCode::BAD_REQUEST);

    let not_an_object = fixture
        .submit("fake", json!({"capability": "ingest_pdf", "input": "r1"}))
        .await;
    assert_eq!(not_an_object.status(), StatusCode::BAD_REQUEST);

    let too_large = fixture
        .submit(
            "fake",
            json!({
                "capability": "ingest_pdf",
                "input": {"blob": "x".repeat(reinforce_plugin_protocol::MAX_INPUT_BYTES)},
            }),
        )
        .await;
    assert_eq!(too_large.status(), StatusCode::BAD_REQUEST);

    // The manifest still describes exactly one plugin.
    let plugins = expect_json(
        json_call(&fixture.app, Method::GET, "/api/plugins", None, None).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(plugins.as_array().map(Vec::len), Some(1));
    assert_eq!(plugins[0]["name"], "fake");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cli_queues_jobs_for_a_running_server() {
    let mut setup = Setup::new(AuthMode::Single);
    setup.plugin("fake", "ok", &[]);
    let manifest = setup.manifest();
    let database = setup.path("test.db");
    std::fs::write(&manifest, json!({"plugins": setup.plugins}).to_string())
        .expect("write manifest");

    let binary = env!("CARGO_BIN_EXE_reinforce");
    let listed = ProcessCommand::new(binary)
        .args(["plugin", "list", "--plugins"])
        .arg(&manifest)
        .output()
        .expect("run plugin list");
    assert!(
        listed.status.success(),
        "plugin list failed: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let plugins: Value = serde_json::from_slice(&listed.stdout).expect("plugin list prints json");
    assert_eq!(plugins[0]["name"], "fake");

    let submitted = ProcessCommand::new(binary)
        .args(["plugin", "submit", "--db"])
        .arg(&database)
        .arg("--plugins")
        .arg(&manifest)
        .args(["--plugin", "fake", "--capability", "ingest_pdf", "--input"])
        .arg(r#"{"resource_id":"r1"}"#)
        .output()
        .expect("run plugin submit");
    assert!(
        submitted.status.success(),
        "plugin submit failed: {}",
        String::from_utf8_lossy(&submitted.stderr)
    );
    let job: Value = serde_json::from_slice(&submitted.stdout).expect("plugin submit prints json");
    assert_eq!(job["status"], "queued");
    let job_id = job["id"].as_str().expect("job id").to_owned();

    let status = ProcessCommand::new(binary)
        .args(["plugin", "status", "--db"])
        .arg(&database)
        .arg("--job")
        .arg(&job_id)
        .output()
        .expect("run plugin status");
    assert!(status.status.success());
    let job: Value = serde_json::from_slice(&status.stdout).expect("status prints json");
    assert_eq!(job["status"], "queued");

    // A server on the same database picks the queued row up.
    let fixture = setup.open().await;
    let job = fixture.wait_terminal(&job_id).await;
    assert_eq!(job["status"], "succeeded", "{job}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_without_a_manifest_leaves_the_queue_alone() {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "reinforce-plugin-nomanifest-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create test directory");
    let directory = TestDir(directory);
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: directory.0.join("test.db"),
        blobs: directory.0.join("blobs"),
        fonts: directory.0.join("fonts"),
        auth: AuthMode::Single,
        plugins: None,
        demo: false,
    };

    let job_id = {
        let mut database = reinforce_core::Database::open(&config.db).expect("open database");
        let job = reinforce_core::PluginJob::queued(
            "fake",
            "ingest_pdf",
            json!({"resource_id": "r1"}),
            1,
        );
        reinforce_core::plugin_store::insert_job(database.connection(), &job).expect("insert");
        reinforce_core::plugin_store::claim_next_queued(database.connection_mut())
            .expect("claim")
            .expect("a queued job");
        job.id
    };

    // `reinforce export` and `reinforce mcp` open the same database without a
    // manifest; they must not settle a queue that belongs to the server.
    let state = AppState::open(&config).await.expect("open state");
    assert!(state.plugins.config().is_empty());
    let job = state
        .plugins
        .job(&job_id)
        .await
        .expect("read job")
        .expect("job exists");
    assert_eq!(job.status, JobStatus::Running);
    assert!(job.error.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_transports_are_refused_at_startup() {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "reinforce-plugin-remote-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create test directory");
    let directory = TestDir(directory);
    let manifest = directory.0.join("plugins.json");
    std::fs::write(
        &manifest,
        json!({"plugins": [{
            "name": "remote",
            "capabilities": ["ingest_pdf"],
            "transport": {"type": "remote", "url": "wss://example.test/plugin"},
        }]})
        .to_string(),
    )
    .expect("write manifest");
    let config = ServeArgs {
        host: "127.0.0.1".to_owned(),
        port: 0,
        db: directory.0.join("test.db"),
        blobs: directory.0.join("blobs"),
        fonts: directory.0.join("fonts"),
        auth: AuthMode::Single,
        plugins: Some(manifest),
        demo: false,
    };
    let error = AppState::open(&config)
        .await
        .err()
        .expect("a remote manifest must not start a server");
    assert!(
        error.message.contains("not implemented"),
        "{}",
        error.message
    );
}
