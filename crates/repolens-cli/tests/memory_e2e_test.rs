//! End-to-end checks of the "typed, decaying memory" claim, driven over
//! JSON-RPC against a real `repolens mcp` process.
//!
//! How it works today (see crates/repolens-memory/src/decay.rs and
//! crates/repolens-app/src/grounding.rs):
//! - Decay is time-based: importance *= 0.5^(whole_days_since_last_access/30)
//!   (30-day half-life), with floors (access count, Confirmed grounding) and
//!   multipliers (supersedes 0.5, contradicts-edge 0.25, Contradicted
//!   grounding 0.5). It runs lazily on recall_longterm / get_summary when the
//!   last run is over an hour old. Nodes below 0.05 are pruned.
//! - recall_longterm ranks by keyword_score * importance.
//! - Grounding is presence-only: an anchor is Confirmed when its file path
//!   (or qualified symbol) exists in the latest scan pack, Contradicted when
//!   it is missing. File *content* is not compared.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

struct Mcp {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Mcp {
    /// Start `repolens mcp` in `cwd`, optionally pinning the memory clock.
    fn start(cwd: &Path, clock: Option<&str>) -> Self {
        let mut cmd = Command::new(bin());
        cmd.arg("mcp")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        match clock {
            Some(t) => cmd.env("REPOLENS_CLOCK_OVERRIDE", t),
            None => cmd.env_remove("REPOLENS_CLOCK_OVERRIDE"),
        };
        let mut child = cmd.spawn().expect("spawn repolens mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut m = Mcp {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        m.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "memory-e2e", "version": "0"}
            }),
        );
        m.send(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        m
    }

    fn send(&mut self, v: serde_json::Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(
            serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        );
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "server closed"
            );
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                if v.get("id") == Some(&serde_json::json!(id)) {
                    return v;
                }
            }
        }
    }

    /// Call a tool and return the envelope's `data` object.
    fn call(&mut self, tool: &str, args: serde_json::Value) -> serde_json::Value {
        let resp = self.request(
            "tools/call",
            serde_json::json!({"name": tool, "arguments": args}),
        );
        assert!(resp.get("error").is_none(), "JSON-RPC error: {resp}");
        assert_ne!(resp["result"]["isError"], serde_json::json!(true), "{resp}");
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        v.get("data").cloned().unwrap_or(v)
    }

    fn memorize(&mut self, content: &str, anchors: &[&str]) -> String {
        let d = self.call(
            "memorize_insight",
            serde_json::json!({"content": content, "type": "DECISION", "importance": 0.5, "anchors": anchors}),
        );
        assert_eq!(d["status"], "created", "{d}");
        d["id"].as_str().unwrap().to_string()
    }

    fn recall(&mut self, query: &str) -> Vec<serde_json::Value> {
        self.call("recall_longterm", serde_json::json!({"query": query}))["results"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_repo(root: &Path) {
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub mod auth;\npub mod cache;\n").unwrap();
    fs::write(
        root.join("src/auth.rs"),
        "pub fn token_ttl_secs() -> u64 { 3600 }\n",
    )
    .unwrap();
    fs::write(root.join("src/cache.rs"), "pub fn size() -> usize { 64 }\n").unwrap();
}

fn scan(root: &Path) {
    let out = Command::new(bin())
        .args(["scan", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn find<'a>(results: &'a [serde_json::Value], id: &str) -> Option<&'a serde_json::Value> {
    results.iter().find(|r| r["id"] == id)
}

const T0: &str = "2026-01-01T00:00:00Z";
const T60: &str = "2026-03-02T00:00:00Z"; // T0 + 60 days

/// (a) Clock-forward: a memory stored 60 days earlier ranks below an
/// otherwise identical fresh one, with importance cut to 1/4 (two half-lives).
#[test]
fn older_memory_ranks_lower_after_clock_moves_forward() {
    let dir = tempfile::tempdir().unwrap();
    write_repo(dir.path());

    let old_id = Mcp::start(dir.path(), Some(T0)).memorize("token rotation policy alpha", &[]);
    let new_id = Mcp::start(dir.path(), Some(T60)).memorize("token rotation policy beta", &[]);

    let results = Mcp::start(dir.path(), Some(T60)).recall("token rotation policy");
    assert_eq!(results.len(), 2, "{results:?}");
    assert_eq!(results[0]["id"], new_id, "fresh memory should rank first");
    assert_eq!(results[1]["id"], old_id);
    let imp = |r: &serde_json::Value| r["importance"].as_f64().unwrap();
    assert!((imp(&results[0]) - 0.5).abs() < 1e-3, "{results:?}");
    assert!((imp(&results[1]) - 0.125).abs() < 1e-3, "{results:?}");
    assert!(results[0]["score"].as_f64() > results[1]["score"].as_f64());
}

/// (b) Grounding + forget, as it works today: Confirmed while the anchored
/// file exists, Contradicted once it is deleted and the repo is rescanned;
/// forget_insight removes a memory for good.
#[test]
fn grounding_contradicted_on_deleted_anchor_and_forget_removes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    scan(root);

    let mut mcp = Mcp::start(root, None);
    let grounded = mcp.memorize("auth tokens expire after one hour", &["src/auth.rs"]);
    let cache = mcp.memorize("auth cache holds 64 entries", &[]);
    let style = mcp.memorize("auth errors use thiserror", &[]);

    let r = mcp.recall("auth");
    assert_eq!(r.len(), 3, "{r:?}");
    assert_eq!(find(&r, &grounded).unwrap()["groundingStatus"], "CONFIRMED");
    assert_eq!(find(&r, &cache).unwrap()["groundingStatus"], "UNCHECKED");
    drop(mcp);

    // Delete the anchored file. Without a rescan the pack still has it.
    fs::remove_file(root.join("src/auth.rs")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub mod cache;\n").unwrap();
    let mut mcp = Mcp::start(root, None);
    let r = mcp.recall("auth");
    assert_eq!(
        find(&r, &grounded).unwrap()["groundingStatus"],
        "CONFIRMED",
        "grounding reads the last scan pack, not the live tree"
    );
    drop(mcp);

    scan(root);
    let mut mcp = Mcp::start(root, None);
    let r = mcp.recall("auth");
    let g = find(&r, &grounded).unwrap();
    assert_eq!(g["groundingStatus"], "CONTRADICTED", "{g}");
    assert_eq!(g["missingAnchors"], serde_json::json!(["src/auth.rs"]));

    // forget_insight: dry run first, then confirm.
    let dry = mcp.call("forget_insight", serde_json::json!({"id": style}));
    assert_eq!(dry["status"], "dry_run", "{dry}");
    assert!(find(&mcp.recall("auth"), &style).is_some());
    let del = mcp.call(
        "forget_insight",
        serde_json::json!({"id": style, "confirm": true}),
    );
    assert_eq!(del["status"], "deleted", "{del}");
    let r = mcp.recall("auth");
    assert!(
        find(&r, &style).is_none(),
        "forgotten memory still recalled"
    );
    assert_eq!(r.len(), 2);
    let raw = fs::read_to_string(root.join(".repolens/memory/memory.json")).unwrap();
    assert!(!raw.contains(&style), "forgotten id still in memory.json");
}

/// FAILS today. The README says memory is "grounded against the current pack"
/// and "Grounds claims against current code". In practice grounding only
/// checks that the anchored path exists: editing the anchored file so the
/// memory is no longer true (and rescanning) still reports `confirmed`.
#[test]
#[ignore = "future: detect content changes"]
fn editing_anchored_file_marks_memory_contradicted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_repo(root);
    scan(root);

    let mut mcp = Mcp::start(root, None);
    let grounded = mcp.memorize("auth tokens expire after one hour", &["src/auth.rs"]);
    assert_eq!(
        find(&mcp.recall("auth tokens"), &grounded).unwrap()["groundingStatus"],
        "CONFIRMED"
    );
    drop(mcp);

    fs::write(
        root.join("src/auth.rs"),
        "pub fn token_ttl_secs() -> u64 { 86400 }\n",
    )
    .unwrap();
    scan(root);

    let mut mcp = Mcp::start(root, None);
    let r = mcp.recall("auth tokens");
    assert_eq!(
        find(&r, &grounded).unwrap()["groundingStatus"],
        "CONTRADICTED",
        "anchored file changed since the memory was stored"
    );
}
