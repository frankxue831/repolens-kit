//! First-run regressions found by the stranger test (fix/first-run):
//! - MCP tools work without `root` (JSON-RPC over stdio).
//! - `repolens init` leaves the root `.gitignore` alone (#39).
//! - Inline Rust tests count as test-bearing files.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_repolens")
}

/// Tiny Rust repo: 6 source files, one (`src/lib.rs`) with inline tests,
/// no `tests/` directory. Marked as a git repo via an empty `.git/` dir.
fn write_repo(root: &Path) {
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub mod a;\npub mod b;\npub mod c;\npub mod d;\npub mod e;\n\
         pub fn add(x: i32, y: i32) -> i32 { x + y }\n\n\
         #[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn adds() { assert_eq!(add(1, 2), 3); }\n}\n",
    )
    .unwrap();
    for m in ["a", "b", "c", "d", "e"] {
        fs::write(
            root.join(format!("src/{m}.rs")),
            format!("pub fn {m}() {{}}\n"),
        )
        .unwrap();
    }
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

/// Minimal JSON-RPC client for `repolens mcp` over stdio.
struct Mcp {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn start(cwd: &Path) -> Self {
        let mut child = Command::new(bin())
            .arg("mcp")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn repolens mcp");
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
                "clientInfo": {"name": "first-run-test", "version": "0"}
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
            let n = self.stdout.read_line(&mut line).unwrap();
            assert!(n > 0, "mcp server closed stdout");
            let v: serde_json::Value = match serde_json::from_str(line.trim()) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("id") == Some(&serde_json::json!(id)) {
                return v;
            }
        }
    }

    fn call(&mut self, tool: &str, args: serde_json::Value) -> serde_json::Value {
        self.request(
            "tools/call",
            serde_json::json!({"name": tool, "arguments": args}),
        )
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tool_text(resp: &serde_json::Value) -> String {
    assert!(resp.get("error").is_none(), "JSON-RPC error: {resp}");
    let result = &resp["result"];
    assert_ne!(
        result["isError"],
        serde_json::json!(true),
        "tool error: {resp}"
    );
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn mcp_get_summary_without_root_uses_server_cwd() {
    let dir = tempfile::tempdir().unwrap();
    write_repo(dir.path());
    scan(dir.path());

    // Start the server from a subdirectory: root should resolve to the repo.
    let mut mcp = Mcp::start(&dir.path().join("src"));
    let resp = mcp.call("get_summary", serde_json::json!({}));
    let text = tool_text(&resp);
    assert!(!text.contains("missing field"), "{text}");
    let v: serde_json::Value = serde_json::from_str(&text).expect("summary JSON");
    assert_eq!(
        v["data"]["project"]["name"],
        serde_json::json!("demo"),
        "{v}"
    );

    // A few other root-taking tools also work without it.
    tool_text(&mcp.call("get_conventions", serde_json::json!({})));
    tool_text(&mcp.call("discover_packs", serde_json::json!({})));
    tool_text(&mcp.call("get_profile", serde_json::json!({})));
}

#[test]
fn mcp_tool_schemas_do_not_require_root() {
    let dir = tempfile::tempdir().unwrap();
    write_repo(dir.path());
    let mut mcp = Mcp::start(dir.path());
    let resp = mcp.request("tools/list", serde_json::json!({}));
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    assert!(!tools.is_empty());
    for t in tools {
        let required = t["inputSchema"]["required"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            !required.iter().any(|r| r == "root"),
            "tool {} still requires root",
            t["name"]
        );
    }
}

#[test]
fn mcp_get_summary_counts_inline_rust_tests() {
    let dir = tempfile::tempdir().unwrap();
    write_repo(dir.path());
    scan(dir.path());
    let mut mcp = Mcp::start(dir.path());
    let resp = mcp.call("get_summary", serde_json::json!({"detail": "standard"}));
    let v: serde_json::Value = serde_json::from_str(&tool_text(&resp)).unwrap();
    assert_eq!(
        v["data"]["stats"]["testFileCount"],
        serde_json::json!(1),
        "{v}"
    );
    assert!(
        v["data"]["stats"]["testToSourceRatio"].as_f64().unwrap() > 0.0,
        "{v}"
    );
}

/// #39: init never edits the root `.gitignore`, in any of its shapes;
/// `.repolens/.gitignore` does the ignoring.
#[test]
fn init_leaves_root_gitignore_byte_identical() {
    for body in [
        "target/",
        "target/\n",
        ".repolens/\n",
        "/.repolens\n",
        "a\r\n.repolens/*\r\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), body).unwrap();
        for _ in 0..2 {
            let out = Command::new(bin())
                .args(["init", "--root", dir.path().to_str().unwrap()])
                .output()
                .unwrap();
            assert!(out.status.success());
        }
        assert_eq!(
            fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
            body
        );
        assert!(dir.path().join(".repolens/.gitignore").is_file());
    }
}

#[test]
fn init_creates_no_root_gitignore() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(bin())
        .args(["init", "--root", dir.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!dir.path().join(".gitignore").exists());
    assert!(dir.path().join(".repolens/.gitignore").is_file());
}

#[test]
fn git_status_clean_after_scan_in_fresh_repo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git available");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    git(&["init", "-q"]);
    fs::remove_dir_all(root.join(".git/hooks")).ok();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.com",
        "commit",
        "-q",
        "-m",
        "init",
    ]);
    assert_eq!(git(&["status", "--porcelain"]), "");

    scan(root);
    assert!(root.join(".repolens/.gitignore").exists());
    assert!(
        !root.join(".gitignore").exists(),
        "scan must not edit user files"
    );
    assert_eq!(
        git(&["status", "--porcelain"]),
        "",
        "status dirty after scan"
    );

    let out = Command::new(bin())
        .arg("findings")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        git(&["status", "--porcelain"]),
        "",
        "status dirty after findings"
    );
}
