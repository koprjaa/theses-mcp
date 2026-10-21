// Project: theses-mcp
// File: src/dispatch.rs
// Description: Turns a tool name and its JSON arguments into a call and a result text.
// Author: Jan Alexandr Kopřiva
// License: MIT

use crate::Theses;
use crate::login::Params;
use serde_json::{Map, Value};

fn arg_str(args: &Map<String, Value>, key: &str) -> Result<String, String> {
    match args.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(format!("{key}\n  Input should be a valid string")),
        None => Err(format!("{key}\n  Field required")),
    }
}

fn arg_opt_str(args: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("{key}\n  Input should be a valid string")),
    }
}

fn arg_int(args: &Map<String, Value>, key: &str, default: i64) -> Result<i64, String> {
    match args.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_i64()
            .ok_or_else(|| format!("{key}\n  Input should be a valid integer")),
    }
}

fn arg_bool(args: &Map<String, Value>, key: &str, default: bool) -> Result<bool, String> {
    match args.get(key) {
        None => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{key}\n  Input should be a valid boolean")),
    }
}

/// Defaults are the ones the schema advertises.
pub fn login_params(args: &Map<String, Value>) -> Result<Params, String> {
    Ok(Params {
        host: arg_opt_str(args, "host")?.unwrap_or_else(|| "theses.cz".into()),
        wait_seconds: arg_int(args, "wait_seconds", 240)?,
        attach: arg_bool(args, "attach", false)?,
        port: arg_int(args, "port", 9222)?,
        url: arg_opt_str(args, "url")?,
    })
}

/// A finished tool call: the text for the client and whether it is an error.
pub struct Reply {
    pub text: String,
    pub is_error: bool,
}

impl Reply {
    pub fn ok(value: &Value) -> Reply {
        // two-space pretty printing, like the Python server's single text block
        Reply {
            text: serde_json::to_string_pretty(value).unwrap_or_default(),
            is_error: false,
        }
    }

    pub fn failed(tool: &str, what: impl std::fmt::Display) -> Reply {
        Reply {
            text: format!("Error executing tool {tool}: {what}"),
            is_error: true,
        }
    }
}

/// Run a tool that needs no browser. `login` is handled by the caller.
pub fn call_blocking(app: &Theses, name: &str, args: &Map<String, Value>) -> Reply {
    let invalid = |e: String| Reply::failed(name, format!("invalid arguments: {e}"));
    let done = |r: crate::error::Result<Value>| match r {
        Ok(v) => Reply::ok(&v),
        Err(e) => Reply::failed(name, e),
    };
    match name {
        "search" => match (arg_str(args, "query"), arg_int(args, "limit", 10)) {
            (Ok(q), Ok(limit)) => done(app.search(&q, limit)),
            (Err(e), _) | (_, Err(e)) => invalid(e),
        },
        "detail" => arg_str(args, "id_or_url").map_or_else(invalid, |id| done(app.detail(&id))),
        "fulltext" => arg_str(args, "id_or_url").map_or_else(invalid, |id| done(app.fulltext(&id))),
        "use_cookie" => match (arg_str(args, "host"), arg_str(args, "cookie")) {
            (Ok(host), Ok(cookie)) => done(app.use_cookie(&host, &cookie)),
            (Err(e), _) | (_, Err(e)) => invalid(e),
        },
        "whoami" => done(app.whoami()),
        _ => Reply {
            text: format!("Unknown tool: {name}"),
            is_error: true,
        },
    }
}
