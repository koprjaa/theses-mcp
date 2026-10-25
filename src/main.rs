// Project: theses-mcp
// File: src/main.rs
// Description: Entry point: serves MCP over stdio, or runs the live self-test with --selftest.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--selftest") {
        let outcome = tokio::task::spawn_blocking(theses_mcp::selftest::run)
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
        return match outcome {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("selftest failed: {e}");
                ExitCode::FAILURE
            }
        };
    }
    match theses_mcp::server::serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("theses-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}
