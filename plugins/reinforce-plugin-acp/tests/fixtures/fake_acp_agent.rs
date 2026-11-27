// Minimal standalone ACP v1 JSON-RPC 2.0 stdio agent fixture for E2E tests.
//
// Compiled once per integration test binary with `rustc` into
// `CARGO_TARGET_TMPDIR`, avoiding any external dependencies or test-only crate
// features while exercising real child processes and stdio pipes.
//
// Usage:
//   fake_acp_agent --mode <mode> [--pid-file <path>] [--scratch <path>]

use std::io::{BufRead, Write};
use std::time::Duration;

fn emit(line: &str) {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(line.as_bytes());
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

fn read_line() -> Option<String> {
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line),
    }
}

fn extract_str(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn extract_id(line: &str) -> String {
    let Some(start) = line.find("\"id\":") else {
        return "0".to_owned();
    };
    let rest = line[start + 5..].trim_start();
    let end = rest
        .find(|ch: char| ch == ',' || ch == '}' || ch.is_whitespace())
        .unwrap_or(rest.len());
    rest[..end].to_owned()
}

fn emit_text_chunk(session_id: &str, escaped_text: &str) {
    emit(&format!(
        "{{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{{\"sessionId\":\"{session_id}\",\"update\":{{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{{\"type\":\"text\",\"text\":\"{escaped_text}\"}}}}}}}}"
    ));
}

fn json_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 16);
    for ch in raw.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

fn valid_proposal_json() -> &'static str {
    concat!(
        "{",
        "\"cards\":[",
        "{",
        "\"kind\":\"knowledge_card\",",
        "\"subject\":\"physics\",",
        "\"topics\":[\"mechanics\"],",
        "\"title\":\"Newton's Third Law\",",
        "\"blocks\":[",
        "{\"type\":\"markdown\",\"text\":\"For every action, there is an equal and opposite reaction.\"},",
        "{\"type\":\"math\",\"latex\":\"\\\\vec{F}_{AB} = -\\\\vec{F}_{BA}\",\"display\":true}",
        "]",
        "},",
        "{",
        "\"kind\":\"question_card\",",
        "\"subject\":\"physics\",",
        "\"topics\":[\"mechanics\"],",
        "\"question_type\":\"single_choice\",",
        "\"stem_blocks\":[{\"type\":\"markdown\",\"text\":\"In an action-reaction pair, the two forces act on:\"}],",
        "\"explanation_blocks\":[{\"type\":\"markdown\",\"text\":\"Third-law pairs always act on two distinct interacting bodies.\"}],",
        "\"options\":[",
        "{\"key\":\"A\",\"content\":\"The same body\"},",
        "{\"key\":\"B\",\"content\":\"Two different interacting bodies\"}",
        "],",
        "\"answer\":[\"B\"],",
        "\"answer_basis\":\"Newton's third law states that forces occur in pairs between two different interacting objects.\"",
        "}",
        "]",
        "}"
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut mode = String::from("ok");
    let mut pid_file: Option<String> = None;
    let mut scratch: Option<String> = None;

    let mut idx = 1;
    while idx < args.len() {
        match args[idx].as_str() {
            "--mode" if idx + 1 < args.len() => {
                mode = args[idx + 1].clone();
                idx += 2;
            }
            "--pid-file" if idx + 1 < args.len() => {
                pid_file = Some(args[idx + 1].clone());
                idx += 2;
            }
            "--scratch" if idx + 1 < args.len() => {
                scratch = Some(args[idx + 1].clone());
                idx += 2;
            }
            _ => {
                idx += 1;
            }
        }
    }

    let session_id = "sess_fake_acp_001";

    // 1. initialize
    let Some(init_line) = read_line() else {
        std::process::exit(1);
    };
    let init_id = extract_id(&init_line);
    if extract_str(&init_line, "method").as_deref() != Some("initialize") {
        eprintln!("expected initialize, got: {init_line}");
        std::process::exit(2);
    }
    if mode == "init_error" {
        emit(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{init_id},\"error\":{{\"code\":-32002,\"message\":\"agent authentication required\"}}}}"
        ));
        return;
    }
    emit(&format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{init_id},\"result\":{{\"protocolVersion\":1,\"agentInfo\":{{\"name\":\"fake-acp-agent\",\"version\":\"0.1.0\"}},\"agentCapabilities\":{{}}}}}}"
    ));

    // 2. session/new
    let Some(new_line) = read_line() else {
        std::process::exit(1);
    };
    let new_id = extract_id(&new_line);
    if extract_str(&new_line, "method").as_deref() != Some("session/new") {
        eprintln!("expected session/new, got: {new_line}");
        std::process::exit(3);
    }
    emit(&format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{new_id},\"result\":{{\"sessionId\":\"{session_id}\"}}}}"
    ));

    if mode == "stall_stdin" {
        if let Some(path) = &pid_file {
            let _ = std::fs::write(path, std::process::id().to_string());
        }
        // Intentionally stop reading stdin so a large session/prompt fills the
        // OS pipe buffer and blocks the plugin's write_frame call.
        std::thread::sleep(Duration::from_secs(15));
        return;
    }

    // 3. session/prompt
    let Some(prompt_line) = read_line() else {
        std::process::exit(1);
    };
    let prompt_id = extract_id(&prompt_line);
    if extract_str(&prompt_line, "method").as_deref() != Some("session/prompt") {
        eprintln!("expected session/prompt, got: {prompt_line}");
        std::process::exit(4);
    }

    if let Some(path) = &pid_file {
        let _ = std::fs::write(path, std::process::id().to_string());
    }

    match mode.as_str() {
        "ok" => {
            // Stream the proposal across two chunks plus a thought chunk to
            // verify streaming reassembly and filtering.
            emit(&format!(
                "{{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{{\"sessionId\":\"{session_id}\",\"update\":{{\"sessionUpdate\":\"agent_thought_chunk\",\"content\":{{\"type\":\"text\",\"text\":\"drafting cards\"}}}}}}}}"
            ));
            let payload = valid_proposal_json();
            let mid = payload.len() / 2;
            emit_text_chunk(session_id, &json_escape(&payload[..mid]));
            emit_text_chunk(session_id, &json_escape(&payload[mid..]));
            emit(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{prompt_id},\"result\":{{\"stopReason\":\"end_turn\"}}}}"
            ));
        }
        "permission_and_fs" => {
            let mut log = String::new();

            // Request permission (must be denied via reject_once option).
            emit(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":101,\"method\":\"session/request_permission\",\"params\":{{\"sessionId\":\"{session_id}\",\"toolCall\":{{\"toolCallId\":\"call_1\",\"title\":\"Write disk\"}},\"options\":[{{\"optionId\":\"allow\",\"name\":\"Allow\",\"kind\":\"allow_once\"}},{{\"optionId\":\"deny\",\"name\":\"Deny\",\"kind\":\"reject_once\"}}]}}}}"
            ));
            if let Some(reply) = read_line() {
                log.push_str(&reply);
            }

            // Attempt fs/read_text_file (must be refused with JSON-RPC error).
            emit("{\"jsonrpc\":\"2.0\",\"id\":102,\"method\":\"fs/read_text_file\",\"params\":{\"sessionId\":\"sess_fake_acp_001\",\"path\":\"/etc/passwd\"}}");
            if let Some(reply) = read_line() {
                log.push_str(&reply);
            }

            // Attempt fs/write_text_file (must be refused with JSON-RPC error).
            emit("{\"jsonrpc\":\"2.0\",\"id\":103,\"method\":\"fs/write_text_file\",\"params\":{\"sessionId\":\"sess_fake_acp_001\",\"path\":\"/tmp/pwn\",\"content\":\"x\"}}");
            if let Some(reply) = read_line() {
                log.push_str(&reply);
            }

            // Attempt terminal/create (must be refused with JSON-RPC error).
            emit("{\"jsonrpc\":\"2.0\",\"id\":104,\"method\":\"terminal/create\",\"params\":{\"sessionId\":\"sess_fake_acp_001\",\"command\":\"sh\"}}");
            if let Some(reply) = read_line() {
                log.push_str(&reply);
            }

            if let Some(path) = &scratch {
                let _ = std::fs::write(path, &log);
            }

            let fenced = format!("```json\n{}\n```", valid_proposal_json());
            emit_text_chunk(session_id, &json_escape(&fenced));
            emit(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{prompt_id},\"result\":{{\"stopReason\":\"end_turn\"}}}}"
            ));
        }
        "prompt_error" => {
            emit(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{prompt_id},\"error\":{{\"code\":-32001,\"message\":\"upstream model quota exceeded\"}}}}"
            ));
        }
        "invalid_json" => {
            emit_text_chunk(
                session_id,
                "Here are some study notes in plain English instead of JSON.",
            );
            emit(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{prompt_id},\"result\":{{\"stopReason\":\"end_turn\"}}}}"
            ));
        }
        "invalid_schema" => {
            let bad = r#"{"cards":[{"kind":"knowledge_card","title":"","blocks":[]}]}"#;
            emit_text_chunk(session_id, &json_escape(bad));
            emit(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{prompt_id},\"result\":{{\"stopReason\":\"end_turn\"}}}}"
            ));
        }
        "oversized" => {
            let chunk = "a".repeat(64 * 1024);
            for _ in 0..12 {
                emit_text_chunk(session_id, &chunk);
            }
            emit(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{prompt_id},\"result\":{{\"stopReason\":\"end_turn\"}}}}"
            ));
        }
        "hang" => loop {
            std::thread::sleep(Duration::from_millis(200));
        },
        "wait_cancel" => {
            while let Some(line) = read_line() {
                if extract_str(&line, "method").as_deref() == Some("session/cancel") {
                    if let Some(path) = &scratch {
                        let _ = std::fs::write(path, line.trim());
                    }
                    emit(&format!(
                        "{{\"jsonrpc\":\"2.0\",\"id\":{prompt_id},\"result\":{{\"stopReason\":\"cancelled\"}}}}"
                    ));
                    return;
                }
            }
        }
        other => {
            eprintln!("unknown fake_acp_agent mode: {other}");
            std::process::exit(64);
        }
    }
}
