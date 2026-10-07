//! G9 finder composition over real IPC (TASK-0147).
//!
//! In-process `agentd` on a sandbox endpoint/database plus the real
//! `agentctl` client library (`finder_request`): query → show →
//! stale through versioned v7 IPC, the trusted [`FinderService`],
//! the lexical engine, and the Git revision backend against a
//! hermetic Git repository (no network, no quota).
//!
//! Unix-only (UDS transport, hermetic `git`). Test-owned sandbox
//! endpoint/database/repo only; production directories are never
//! touched.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ipc_contracts::proto::{
    finder_core_request, finder_core_response, FinderCoreRequest, FinderCoreResponse,
    FinderQueryRequest, FinderShowRequest,
};
use ipc_transport::platform::unix::{UnixEndpoint, SOCKET_FILE_NAME};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Sandbox {
    root: PathBuf,
    endpoint: UnixEndpoint,
    db: PathBuf,
    repo: PathBuf,
}

impl Sandbox {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from("/tmp").join(format!(
            "g9-finder-{}-{}-{suite}-{id}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("sandbox root");
        // Neither root/db nor root/run is pre-created: the daemon owns
        // its 0700 data directory (creating it) and its 0700 endpoint
        // directory, failing closed on unexpected pre-existing objects.
        let endpoint = UnixEndpoint {
            socket_path: root.join("run").join(SOCKET_FILE_NAME),
            runtime_dir: root.join("run"),
        };
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join("src")).expect("sandbox repo dir");
        std::fs::write(
            repo.join("src/main.rs"),
            "fn main() {\n    println!(\"composition_needle\");\n}\n",
        )
        .expect("sandbox main.rs");
        std::fs::write(repo.join("notes.md"), "# notes\n\nnothing here\n")
            .expect("sandbox notes.md");
        git(&repo, &["init", "-q"]);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        Self {
            db: root.join("db").join("state.sqlite3"),
            root,
            endpoint,
            repo,
        }
    }

    fn head(&self) -> String {
        git_out(&self.repo, &["rev-parse", "HEAD"])
    }

    fn commit(&self, name: &str, body: &[u8], message: &str) {
        std::fs::write(self.repo.join(name), body).expect("sandbox write");
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-q", "-m", message]);
    }

    fn spawn_daemon(
        &self,
    ) -> tokio::task::JoinHandle<Result<agentd::RunOutcome, agentd::DaemonError>> {
        let endpoint = self.endpoint.clone();
        let db = self.db.clone();
        tokio::spawn(async move {
            let config = agentd::DaemonConfig::with_execution(db);
            agentd::run_on_endpoint(&endpoint, config).await
        })
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git(repo: &Path, args: &[&str]) {
    let _ = git_out(repo, args);
}

fn git_out(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("fixture git");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout)
        .expect("git stdout")
        .trim()
        .to_owned()
}

fn inert_spawn() -> std::io::Result<std::process::Child> {
    Command::new("true").spawn()
}

async fn wait_ready(endpoint: &UnixEndpoint) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if agentctl::connect_and_handshake(endpoint).await.is_ok() {
            return;
        }
        if Instant::now() >= deadline {
            panic!("daemon never became ready");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn finder(endpoint: &UnixEndpoint, request: FinderCoreRequest) -> FinderCoreResponse {
    agentctl::finder_request(endpoint, inert_spawn, request)
        .await
        .expect("finder exchange")
}

fn query_request(repo: &Sandbox, text: &str, limit: u32) -> FinderCoreRequest {
    FinderCoreRequest {
        payload: Some(finder_core_request::Payload::Query(FinderQueryRequest {
            repository_path: repo.repo.to_string_lossy().to_string(),
            text: text.to_owned(),
            scope: String::new(),
            limit,
            continuation: String::new(),
        })),
    }
}

fn page_of(response: &FinderCoreResponse) -> &ipc_contracts::proto::FinderPageResponse {
    match &response.payload {
        Some(finder_core_response::Payload::Page(page)) => page,
        other => panic!("expected page payload, got {other:?}"),
    }
}

fn error_of(response: &FinderCoreResponse) -> &ipc_contracts::proto::FinderErrorResponse {
    match &response.payload {
        Some(finder_core_response::Payload::Error(error)) => error,
        other => panic!("expected error payload, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn query_returns_bounded_page_bound_to_head() {
    let sandbox = Sandbox::new("query-page");
    let daemon = sandbox.spawn_daemon();
    wait_ready(&sandbox.endpoint).await;
    let response = finder(
        &sandbox.endpoint,
        query_request(&sandbox, "composition_needle", 10),
    )
    .await;
    let page = page_of(&response);
    assert_eq!(page.revision, sandbox.head());
    assert!(!page.dirty);
    assert_eq!(page.backend_name, "finder-local");
    assert!(
        page.hits
            .iter()
            .any(|hit| hit.path == "src/main.rs" && hit.excerpt.contains("composition_needle")),
        "must hit src/main.rs, got {:?}",
        page.hits
    );
    assert!(page.total_hits as usize >= page.hits.len());
    assert!(page.continuation.is_empty());
    daemon.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn show_reopens_lines_over_ipc() {
    let sandbox = Sandbox::new("show-span");
    let daemon = sandbox.spawn_daemon();
    wait_ready(&sandbox.endpoint).await;
    let first = finder(
        &sandbox.endpoint,
        query_request(&sandbox, "composition_needle", 10),
    )
    .await;
    let page = page_of(&first).clone();
    let request = FinderCoreRequest {
        payload: Some(finder_core_request::Payload::Show(FinderShowRequest {
            repository_path: sandbox.repo.to_string_lossy().to_string(),
            evidence_id: page.hits[0].evidence_id.clone(),
            context_before: 0,
            context_after: 0,
        })),
    };
    let response = finder(&sandbox.endpoint, request).await;
    match &response.payload {
        Some(finder_core_response::Payload::Span(span)) => {
            assert_eq!(span.path, "src/main.rs");
            assert!(span
                .lines
                .iter()
                .any(|line| line.contains("composition_needle")));
            assert!(!span.content_hash.is_empty());
            assert_eq!(span.revision, sandbox.head());
        }
        other => panic!("expected span payload, got {other:?}"),
    }
    daemon.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn show_with_context_widens_window_over_ipc() {
    // main.rs is three lines with the needle on line 2; context 1
    // widens the reopened window to the full file over real IPC.
    let sandbox = Sandbox::new("show-context");
    let daemon = sandbox.spawn_daemon();
    wait_ready(&sandbox.endpoint).await;
    let first = finder(
        &sandbox.endpoint,
        query_request(&sandbox, "composition_needle", 10),
    )
    .await;
    let page = page_of(&first).clone();
    let request = FinderCoreRequest {
        payload: Some(finder_core_request::Payload::Show(FinderShowRequest {
            repository_path: sandbox.repo.to_string_lossy().to_string(),
            evidence_id: page.hits[0].evidence_id.clone(),
            context_before: 1,
            context_after: 1,
        })),
    };
    let response = finder(&sandbox.endpoint, request).await;
    match &response.payload {
        Some(finder_core_response::Payload::Span(span)) => {
            assert_eq!(span.path, "src/main.rs");
            assert_eq!((span.start_line, span.end_line), (2, 2));
            assert_eq!((span.window_start_line, span.window_end_line), (1, 3));
            assert_eq!(span.lines.len(), 3);
            assert!(span.lines[0].contains("fn main"));
            assert!(span.lines[1].contains("composition_needle"));
            assert!(!span.content_hash.is_empty());
        }
        other => panic!("expected span payload, got {other:?}"),
    }
    daemon.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn stale_after_new_commit_over_ipc() {
    let sandbox = Sandbox::new("stale-commit");
    let daemon = sandbox.spawn_daemon();
    wait_ready(&sandbox.endpoint).await;
    let first = finder(
        &sandbox.endpoint,
        query_request(&sandbox, "composition_needle", 10),
    )
    .await;
    let page = page_of(&first).clone();
    let before = page.revision.clone();
    sandbox.commit("extra.rs", b"more\n", "second");
    let after = sandbox.head();
    assert_ne!(before, after);
    let request = FinderCoreRequest {
        payload: Some(finder_core_request::Payload::Show(FinderShowRequest {
            repository_path: sandbox.repo.to_string_lossy().to_string(),
            evidence_id: page.hits[0].evidence_id.clone(),
            context_before: 0,
            context_after: 0,
        })),
    };
    let response = finder(&sandbox.endpoint, request).await;
    let error = error_of(&response);
    assert_eq!(error.code, "stale");
    assert_eq!(error.expected_rev, before);
    assert_eq!(error.actual_rev, after);
    daemon.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn rejects_malformed_token_and_paths_over_ipc() {
    let sandbox = Sandbox::new("rejects-bad-input");
    let daemon = sandbox.spawn_daemon();
    wait_ready(&sandbox.endpoint).await;
    let mut bad_token = query_request(&sandbox, "composition_needle", 10);
    match bad_token.payload.as_mut().unwrap() {
        finder_core_request::Payload::Query(query) => {
            query.continuation = "garbage".to_owned();
        }
        _ => unreachable!(),
    }
    let response = finder(&sandbox.endpoint, bad_token).await;
    let error = error_of(&response);
    assert_eq!(error.code, "invalid_continuation");
    let mut bad_root = query_request(&sandbox, "composition_needle", 10);
    match bad_root.payload.as_mut().unwrap() {
        finder_core_request::Payload::Query(query) => {
            query.repository_path = "repo/sub".to_owned();
        }
        _ => unreachable!(),
    }
    let response = finder(&sandbox.endpoint, bad_root).await;
    let error = error_of(&response);
    assert_eq!(error.code, "invalid_input");
    let mut missing = query_request(&sandbox, "composition_needle", 10);
    match missing.payload.as_mut().unwrap() {
        finder_core_request::Payload::Query(query) => {
            query.repository_path = sandbox.root.join("nope").to_string_lossy().to_string();
        }
        _ => unreachable!(),
    }
    let response = finder(&sandbox.endpoint, missing).await;
    let error = error_of(&response);
    assert_eq!(error.code, "not_a_repository");
    daemon.abort();
}
