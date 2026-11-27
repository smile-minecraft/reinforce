//! End-to-end tests with fake LaTeX and dvisvgm executables: the server loads
//! the real plugin binary and turns its SVG operations into a pending proposal.

use std::path::Path;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use reinforce_server::config::{AuthMode, ServeArgs};
use reinforce_server::state::AppState;

const PLUGIN_BINARY: &str = env!("CARGO_BIN_EXE_reinforce-plugin-tikz");

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
    source: String,
}

impl Fixture {
    async fn open(latex: &Path, dvisvgm: &Path) -> Self {
        let directory = tempfile::tempdir().expect("test directory");
        let manifest = directory.path().join("plugins.json");
        std::fs::write(
            &manifest,
            json!({"plugins": [{
                "name": "tikz",
                "capabilities": ["tikz_render"],
                "transport": {
                    "type": "stdio",
                    "command": PLUGIN_BINARY,
                    "args": [
                        "--latex", latex.display().to_string(),
                        "--dvisvgm", dvisvgm.display().to_string(),
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
            source: "\\begin{tikzpicture}\\draw (0,0) -- (1,1);\\end{tikzpicture}".to_owned(),
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
                "/api/plugins/tikz/jobs",
                Some(json!({
                    "capability": "tikz_render",
                    "input": {"source": self.source, "title": "Triangle"},
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

fn fake_latex(path: &Path) {
    write_executable(
        path,
        r#"#!/bin/sh
if [ "$openin_any" != "p" ] || [ "$openout_any" != "p" ]; then
  echo "kpathsea file access is not restricted" >&2
  exit 1
fi
case "$(basename "$(pwd)")" in
  tikz-*) ;;
  *) echo "latex must run inside the job directory, not $(pwd)" >&2; exit 1 ;;
esac
directory=""
tex=""
while [ $# -gt 0 ]; do
  case "$1" in
    -output-directory=*) directory="${1#*=}"; shift ;;
    /*) echo "latex must only get relative paths, got $1" >&2; exit 1 ;;
    *.tex) tex="$1"; shift ;;
    *) shift ;;
  esac
done
base=$(basename "$tex" .tex)
printf 'fake dvi\n' > "$directory/$base.dvi"
"#,
    );
}

fn fake_dvisvgm(path: &Path) {
    write_executable(
        path,
        r#"#!/bin/sh
svg=""
while [ $# -gt 0 ]; do
  case "$1" in
    -o) svg="$2"; shift 2 ;;
    *) shift ;;
  esac
done
cat > "$svg" <<'EOF'
<svg xmlns="http://www.w3.org/2000/svg" width="120pt" height="80pt" viewBox="0 0 120 80"><path d="M0 0 L1 1"/></svg>
EOF
"#,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tikz_job_becomes_a_pending_proposal() {
    let directory = tempfile::tempdir().expect("tool directory");
    let latex = directory.path().join("latex");
    let dvisvgm = directory.path().join("dvisvgm");
    fake_latex(&latex);
    fake_dvisvgm(&dvisvgm);
    let fixture = Fixture::open(&latex, &dvisvgm).await;

    let (status, plugins) = fixture.json_call(Method::GET, "/api/plugins", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(plugins[0]["name"], "tikz");
    assert_eq!(plugins[0]["capabilities"][0], "tikz_render");

    let id = fixture.submit().await;
    let job = fixture.wait_terminal(&id).await;
    assert_eq!(job["status"], "succeeded", "{job}");

    let proposal_id = job["proposal_id"].as_str().expect("proposal id");
    let (status, proposal) = fixture
        .json_call(Method::GET, &format!("/api/proposals/{proposal_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{proposal}");
    assert_eq!(proposal["status"], "pending");
    assert_eq!(proposal["submitted_by"], "agent:plugin:tikz");
    let operations = proposal["payload"]["operations"]
        .as_array()
        .expect("proposal operations");
    assert_eq!(operations.len(), 2);
    assert_eq!(operations[0]["op"], "resource");
    assert_eq!(operations[0]["data"]["kind"], "image");
    assert_eq!(operations[0]["data"]["width"], 120);
    assert_eq!(operations[0]["data"]["height"], 80);
    assert_eq!(operations[1]["op"], "knowledge_card");
    assert_eq!(operations[1]["data"]["title"], "Triangle");
    assert_eq!(operations[1]["data"]["blocks"][0]["syntax"], "tikz");
    assert!(
        operations[1]["data"]["blocks"][0]["svg_cache"]
            .as_str()
            .expect("svg cache")
            .contains("<svg")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_dvisvgm_reports_a_structured_error() {
    let directory = tempfile::tempdir().expect("tool directory");
    let latex = directory.path().join("latex");
    let dvisvgm = directory.path().join("missing-dvisvgm");
    fake_latex(&latex);
    let fixture = Fixture::open(&latex, &dvisvgm).await;

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
