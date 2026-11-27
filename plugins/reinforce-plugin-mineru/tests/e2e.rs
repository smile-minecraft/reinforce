//! End-to-end tests: the real server loads the real plugin binary and runs it
//! against a fake MinerU script, so the queue, handshake, proposal hand-off and
//! missing-tool error path are all exercised.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use reinforce_server::config::{AuthMode, ServeArgs};
use reinforce_server::state::AppState;

const PLUGIN_BINARY: &str = env!("CARGO_BIN_EXE_reinforce-plugin-mineru");

fn write_executable(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write fake tool");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)
        .expect("stat fake tool")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("make fake tool executable");
}

struct Fixture {
    app: Router<()>,
    _directory: tempfile::TempDir,
    pdf: PathBuf,
}

impl Fixture {
    async fn open(tool: &Path) -> Self {
        let directory = tempfile::tempdir().expect("test directory");
        let pdf = directory.path().join("paper.pdf");
        std::fs::write(&pdf, b"%PDF-1.4\nprobe\n").expect("write pdf");
        let manifest = directory.path().join("plugins.json");
        std::fs::write(
            &manifest,
            json!({"plugins": [{
                "name": "mineru",
                "capabilities": ["ingest_pdf"],
                "transport": {
                    "type": "stdio",
                    "command": PLUGIN_BINARY,
                    "args": [
                        "--mineru", tool.display().to_string(),
                        "--work-dir", directory.path().display().to_string(),
                    ],
                },
                "timeout_secs": 30,
                "max_output_bytes": 1_048_576,
                "max_attempts": 1,
            }]})
            .to_string(),
        )
        .expect("write manifest");
        let config = ServeArgs {
            host: "127.0.0.1".to_owned(),
            port: 0,
            db: directory.path().join("test.db"),
            blobs: directory.path().join("blobs"),
            fonts: directory.path().join("fonts"),
            auth: AuthMode::Single,
            plugins: Some(manifest),
            demo: false,
        };
        let state = AppState::open(&config).await.expect("open server state");
        Self {
            app: reinforce_server::app(state),
            _directory: directory,
            pdf,
        }
    }

    async fn json_call(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "127.0.0.1")
            .header(header::CONTENT_TYPE, "application/json");
        let bytes = body
            .map(|value| serde_json::to_vec(&value).expect("encode request"))
            .unwrap_or_default();
        let request = builder.body(Body::from(bytes)).expect("build request");
        let response = self
            .app
            .clone()
            .oneshot(request)
            .await
            .expect("run request");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read response");
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    async fn submit(&self) -> String {
        let (status, job) = self
            .json_call(
                Method::POST,
                "/api/plugins/mineru/jobs",
                Some(json!({
                    "capability": "ingest_pdf",
                    "input": {"pdf_path": self.pdf.display().to_string()},
                })),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{job}");
        job["id"].as_str().expect("job id").to_owned()
    }

    async fn wait_terminal(&self, id: &str) -> Value {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let (status, job) = self
                .json_call(Method::GET, &format!("/api/plugins/jobs/{id}"), None)
                .await;
            assert_eq!(status, StatusCode::OK, "{job}");
            if matches!(
                job["status"].as_str(),
                Some("succeeded") | Some("failed") | Some("cancelled")
            ) {
                return job;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "job never finished: {job}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

fn fake_mineru(path: &Path) {
    write_executable(
        path,
        r#"#!/bin/sh
output=""
while [ $# -gt 0 ]; do
  case "$1" in
    --output) output="$2"; shift 2 ;;
    *) shift ;;
  esac
done
cat > "$output" <<'EOF'
<!-- page 1 of 1 -->
# Probe findings
MinerU extracted text from the page.
EOF
printf '{"parse":{"status":"done","sha256":"abc"},"content":null}\n'
"#,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mineru_job_becomes_a_pending_proposal() {
    let directory = tempfile::tempdir().expect("tool directory");
    let tool = directory.path().join("mineru");
    fake_mineru(&tool);
    let fixture = Fixture::open(&tool).await;

    let (status, plugins) = fixture.json_call(Method::GET, "/api/plugins", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(plugins[0]["name"], "mineru");
    assert_eq!(plugins[0]["transport"], "stdio");

    let id = fixture.submit().await;
    let job = fixture.wait_terminal(&id).await;
    assert_eq!(job["status"], "succeeded", "{job}");
    assert_eq!(job["progress"], 1.0);

    let proposal_id = job["proposal_id"].as_str().expect("proposal id");
    let (status, proposal) = fixture
        .json_call(Method::GET, &format!("/api/proposals/{proposal_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{proposal}");
    assert_eq!(proposal["status"], "pending");
    assert_eq!(proposal["submitted_by"], "agent:plugin:mineru");
    let operations = proposal["payload"]["operations"]
        .as_array()
        .expect("proposal operations");
    assert_eq!(operations.len(), 3);
    assert_eq!(operations[0]["op"], "resource");
    assert_eq!(operations[0]["data"]["kind"], "pdf");
    assert_eq!(operations[1]["op"], "anchor");
    assert_eq!(operations[1]["data"]["pdf_page"], 1);
    assert_eq!(
        operations[1]["data"]["resource_id"],
        operations[0]["data"]["id"]
    );
    assert_eq!(operations[2]["op"], "knowledge_card");
    assert_eq!(
        operations[2]["data"]["anchor_ids"][0],
        operations[1]["data"]["id"]
    );
    assert_eq!(operations[2]["data"]["verification"], "unchecked");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_mineru_reports_a_structured_error() {
    let directory = tempfile::tempdir().expect("tool directory");
    let tool = directory.path().join("missing-mineru");
    let fixture = Fixture::open(&tool).await;

    let id = fixture.submit().await;
    let job = fixture.wait_terminal(&id).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["error"]["code"], "plugin_reported", "{job}");
    assert_eq!(
        job["error"]["details"]["plugin_code"], "external_tool_missing",
        "{job}"
    );
    assert!(job["proposal_id"].is_null());
}
