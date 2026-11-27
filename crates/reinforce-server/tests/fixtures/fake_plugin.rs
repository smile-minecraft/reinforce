// Minimal stdio plugin used by the server integration tests.
//
// It is compiled on demand with `rustc` by `tests/plugins.rs` so the tests need
// no extra build step, no network and no test-only crate feature. It speaks the
// same newline-delimited JSON protocol as a real plugin, but it only needs to
// find the handful of string fields the host sends in the handshake.
//
// Usage: fake_plugin <mode> [scratch-path]

use std::io::{BufRead, Write};
use std::time::Duration;

fn field(line: &str, name: &str) -> String {
    let needle = format!("\"{name}\":\"");
    let Some(start) = line.find(&needle) else {
        return String::new();
    };
    let rest = &line[start + needle.len()..];
    let end = rest.find('"').unwrap_or(rest.len());
    rest[..end].to_owned()
}

fn emit(text: &str) {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

fn ready(job: &str, plugin: &str, capability: &str, version: u32) {
    emit(&format!(
        "{{\"type\":\"ready\",\"protocol\":\"reinforce_plugin\",\"version\":{version},\
         \"job_id\":\"{job}\",\"plugin\":\"{plugin}\",\"capabilities\":[\"{capability}\"]}}"
    ));
}

fn result(job: &str, operations: &str) {
    emit(&format!(
        "{{\"type\":\"result\",\"job_id\":\"{job}\",\"operations\":[{operations}]}}"
    ));
}

/// A knowledge card that claims to be published and verified. The server must
/// force `unchecked` and keep it pending.
fn published_card() -> String {
    concat!(
        "{\"op\":\"knowledge_card\",\"temp_ref\":\"$card\",",
        "\"data\":{\"id\":\"$card\",\"subject\":\"physics\",\"topics\":[\"mechanics\"],",
        "\"title\":\"Drafted by a plugin\",",
        "\"blocks\":[{\"type\":\"markdown\",\"text\":\"plugin output\"}],",
        "\"anchor_ids\":[],\"status\":\"published\",\"verification\":\"verified\",",
        "\"unsure_count\":0,\"metadata\":{},",
        "\"created_at\":\"2026-11-27T00:00:00Z\",\"updated_at\":\"2026-11-27T00:00:00Z\"}}"
    )
    .to_owned()
}

/// A batch that fails referential validation: the anchor does not exist.
fn invalid_card() -> String {
    concat!(
        "{\"op\":\"knowledge_card\",\"temp_ref\":\"$card\",",
        "\"data\":{\"id\":\"$card\",\"subject\":\"physics\",\"topics\":[],",
        "\"title\":\"Broken reference\",\"blocks\":[],",
        "\"anchor_ids\":[\"01a21714-c98c-7bfe-8cf8-a60b84573f56\"],",
        "\"status\":\"draft\",\"verification\":\"unchecked\",\"unsure_count\":0,\"metadata\":{},",
        "\"created_at\":\"2026-11-27T00:00:00Z\",\"updated_at\":\"2026-11-27T00:00:00Z\"}}"
    )
    .to_owned()
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let mode = arguments.get(1).map(String::as_str).unwrap_or("ok");
    let scratch = arguments.get(2).cloned().unwrap_or_default();

    if mode == "crash_before_ready" {
        std::process::exit(2);
    }

    let mut hello = String::new();
    if std::io::stdin().lock().read_line(&mut hello).is_err() {
        std::process::exit(1);
    }
    let job = field(&hello, "job_id");
    let plugin = field(&hello, "plugin");
    let capability = field(&hello, "capability");

    match mode {
        "hang" | "slow" => {
            if !scratch.is_empty() {
                let _ = std::fs::write(&scratch, std::process::id().to_string());
            }
            ready(&job, &plugin, &capability, 1);
            if mode == "slow" {
                emit(&format!(
                    "{{\"type\":\"progress\",\"job_id\":\"{job}\",\"fraction\":0.1,\"message\":\"working\"}}"
                ));
            }
            loop {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        "ok" => {
            ready(&job, &plugin, &capability, 1);
            emit(&format!(
                "{{\"type\":\"progress\",\"job_id\":\"{job}\",\"fraction\":0.5,\"message\":\"halfway\"}}"
            ));
            result(&job, &published_card());
        }
        "retry_once" => {
            if std::path::Path::new(&scratch).exists() {
                ready(&job, &plugin, &capability, 1);
                result(&job, &published_card());
            } else {
                let _ = std::fs::write(&scratch, "first attempt");
                std::process::exit(4);
            }
        }
        "error" => {
            ready(&job, &plugin, &capability, 1);
            emit(&format!(
                "{{\"type\":\"error\",\"job_id\":\"{job}\",\"code\":\"unsupported_page\",\
                 \"message\":\"page 12 has no text layer\"}}"
            ));
        }
        "crash" => {
            ready(&job, &plugin, &capability, 1);
            eprintln!("fake plugin crashed on purpose");
            std::process::exit(3);
        }
        "overflow" => {
            ready(&job, &plugin, &capability, 1);
            let noise = "x".repeat(8 * 1024);
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(noise.as_bytes());
            let _ = stdout.flush();
            std::thread::sleep(Duration::from_secs(1));
            std::process::exit(0);
        }
        "flood" => {
            // Endless output with no newline: the host must stop reading at
            // its byte cap instead of buffering until the timeout.
            ready(&job, &plugin, &capability, 1);
            let noise = "x".repeat(64 * 1024);
            let mut stdout = std::io::stdout();
            loop {
                if stdout.write_all(noise.as_bytes()).is_err() {
                    std::process::exit(0);
                }
            }
        }
        "bad_version" => {
            ready(&job, &plugin, &capability, 99);
            std::thread::sleep(Duration::from_millis(200));
        }
        "bad_json" => {
            ready(&job, &plugin, &capability, 1);
            emit("this is not a protocol message");
        }
        "wrong_job_id" => {
            ready(&job, &plugin, &capability, 1);
            result("01a21714-c98c-7bfe-8cf8-a60b84573f56", &published_card());
        }
        "invalid_batch" => {
            ready(&job, &plugin, &capability, 1);
            result(&job, &invalid_card());
        }
        other => {
            eprintln!("unknown fake plugin mode {other}");
            std::process::exit(64);
        }
    }
}
