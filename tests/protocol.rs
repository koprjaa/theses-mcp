// Project: theses-mcp
// File: tests/protocol.rs
// Description: Start the real binary and speak MCP to it over stdio, against what the Python server answered.
// Author: Jan Alexandr Kopřiva
// License: MIT

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn python() -> Value {
    serde_json::from_str(include_str!("fixtures/protocol.json")).unwrap()
}

/// Send the requests, collect one response per id, and stop the server.
fn converse(requests: &[Value]) -> Vec<Value> {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_theses-mcp"))
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env_remove("THESES_COOKIES")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                let _ = tx.send(v);
            }
        }
    });
    let wanted = requests.iter().filter(|r| r.get("id").is_some()).count();
    for r in requests {
        writeln!(stdin, "{r}").unwrap();
        stdin.flush().unwrap();
        if r["method"] == "initialize" {
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    let mut got = Vec::new();
    while got.len() < wanted {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(v) => got.push(v),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(got.len(), wanted, "the server went quiet: {got:?}");
    got
}

fn by_id(got: &[Value], id: u64) -> &Value {
    got.iter()
        .find(|v| v["id"] == id)
        .unwrap_or_else(|| panic!("no response {id}"))
}

fn session() -> Vec<Value> {
    let call = |id: u64, name: &str, args: Value| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}});
    vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        call(3, "whoami", json!({})),
        call(4, "search", json!({"query":"x","limit":0})),
        call(5, "nosuch", json!({})),
        call(6, "search", json!({})),
        call(7, "detail", json!({"id_or_url": 5})),
    ]
}

#[test]
fn the_binary_speaks_mcp_like_the_python_server() {
    let got = converse(&session());
    let py = python();

    let init = &by_id(&got, 1)["result"];
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert_eq!(init["serverInfo"]["name"], "theses");
    assert!(init["capabilities"]["tools"].is_object());

    // names, descriptions and input schemas, byte for byte what Python advertised
    assert_eq!(by_id(&got, 2)["result"], py["tools_list"]);

    for (id, key) in [(3, "whoami"), (4, "search_limit0"), (5, "unknown_tool")] {
        let (rs, pyr) = (&by_id(&got, id)["result"], &py[key]);
        assert_eq!(rs["content"], pyr["content"], "{key}");
        assert_eq!(rs["isError"], pyr["isError"], "{key}");
    }
}

#[test]
fn bad_arguments_are_a_tool_error_not_a_protocol_error() {
    let got = converse(&session());
    for id in [6, 7] {
        let r = &by_id(&got, id)["result"];
        assert_eq!(r["isError"], true, "{r}");
        assert!(
            r["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("Error executing tool")
        );
    }
}

#[test]
fn the_six_tools_have_the_documented_defaults() {
    let tools = &python()["tools_list"]["tools"];
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "search",
            "login",
            "use_cookie",
            "whoami",
            "detail",
            "fulltext"
        ]
    );
    assert_eq!(
        tools[0]["inputSchema"]["properties"]["limit"]["default"],
        10
    );
    assert_eq!(
        tools[1]["inputSchema"]["properties"]["wait_seconds"]["default"],
        240
    );
    assert_eq!(
        tools[1]["inputSchema"]["properties"]["port"]["default"],
        9222
    );
    assert_eq!(
        tools[1]["inputSchema"]["properties"]["host"]["default"],
        "theses.cz"
    );
}
