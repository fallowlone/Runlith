//! Live numbers from GitHub for metric widgets (TASK-0230).
//!
//! The webview has no network (CSP `connect-src ipc:`); this is the only place
//! the desktop host reaches out, and only to one host: the base URL is a
//! constant, the path comes from a validated `MetricSource`, and redirects
//! never leave it. What leaves the device is the repository name, which the
//! widget says on its card (ADR-0014).
//!
//! Anonymous GitHub allows 60 requests per hour per address and counts every
//! poll, so one lock serializes requests, a source is never asked twice inside
//! its refresh interval, and a rate limit stops all sources until it resets.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use reqwest::header::{HeaderMap, ACCEPT, ETAG, IF_NONE_MATCH, LINK, RETRY_AFTER};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use ts_rs::TS;
use widget_spec::{MetricSource, RefreshSeconds, SourceType};

use crate::dashboard::write_atomic;

const API_HOST: &str = "api.github.com";
const API_BASE: &str = "https://api.github.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// GitHub asks for at least a minute after a secondary limit without a header.
const DEFAULT_LIMIT_WAIT_SECONDS: u64 = 60;
/// Timers drift: a poll a moment early must still count as due.
const INTERVAL_SLACK_MS: u64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MetricState {
    Fresh,
    Offline,
    RateLimited,
    NotFound,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MetricDto {
    #[ts(type = "number | null")]
    pub value: Option<u64>,
    #[ts(type = "number | null")]
    pub fetched_at_ms: Option<u64>,
    pub state: MetricState,
    #[ts(type = "number | null")]
    pub retry_at_ms: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
enum Fetched {
    Value { value: u64, etag: Option<String> },
    NotModified,
    NotFound,
    RateLimited { wait_seconds: u64 },
    Unreachable,
}

fn url_for(base: &str, source: &MetricSource) -> String {
    let repo = source.repo.as_str();
    match source.kind {
        SourceType::GithubRepoStars => format!("{base}/repos/{repo}"),
        SourceType::GithubRepoOpenPrs => {
            format!("{base}/repos/{repo}/pulls?state=open&per_page=1")
        }
    }
}

/// `open_issues_count` also counts pull requests' issues, so it cannot answer
/// this: with one item per page the last page number is the count.
fn last_page(link: &str) -> Option<u64> {
    let entry = link.split(',').find(|part| part.contains("rel=\"last\""))?;
    let url = entry.split('<').nth(1)?.split('>').next()?;
    let query = url.split_once('?')?.1;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("page="))?
        .parse()
        .ok()
}

fn open_prs_from(link: Option<&str>, body: &[u8]) -> Option<u64> {
    if let Some(count) = link.and_then(last_page) {
        return Some(count);
    }
    // No `last` link: everything fits on this one page.
    let items: serde_json::Value = serde_json::from_slice(body).ok()?;
    items.as_array().map(|list| list.len() as u64)
}

fn stars_from(body: &[u8]) -> Option<u64> {
    let repository: serde_json::Value = serde_json::from_slice(body).ok()?;
    repository.get("stargazers_count")?.as_u64()
}

fn limit_wait(status: u16, headers: &HeaderMap, now_seconds: u64) -> Option<u64> {
    if status != 403 && status != 429 {
        return None;
    }
    let text = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    if let Some(seconds) = text(RETRY_AFTER.as_str()).and_then(|v| v.parse().ok()) {
        return Some(seconds);
    }
    if text("x-ratelimit-remaining") == Some("0") {
        let reset: u64 = text("x-ratelimit-reset")?.parse().ok()?;
        return Some(reset.saturating_sub(now_seconds).max(1));
    }
    (status == 429).then_some(DEFAULT_LIMIT_WAIT_SECONDS)
}

async fn bounded_body(mut response: reqwest::Response) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BODY_BYTES {
            return None;
        }
    }
    Some(body)
}

async fn fetch(
    client: &reqwest::Client,
    base: &str,
    source: &MetricSource,
    etag: Option<&str>,
    now_seconds: u64,
) -> Fetched {
    let mut request = client
        .get(url_for(base, source))
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(etag) = etag {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let Ok(response) = request.send().await else {
        return Fetched::Unreachable;
    };
    let status = response.status().as_u16();
    match status {
        304 => return Fetched::NotModified,
        404 => return Fetched::NotFound,
        _ => {}
    }
    if let Some(wait_seconds) = limit_wait(status, response.headers(), now_seconds) {
        return Fetched::RateLimited { wait_seconds };
    }
    if status != 200 {
        return Fetched::Unreachable;
    }
    let header = |name| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let (etag, link) = (header(ETAG), header(LINK));
    let Some(body) = bounded_body(response).await else {
        return Fetched::Unreachable;
    };
    let value = match source.kind {
        SourceType::GithubRepoStars => stars_from(&body),
        SourceType::GithubRepoOpenPrs => open_prs_from(link.as_deref(), &body),
    };
    match value {
        Some(value) => Fetched::Value { value, etag },
        None => Fetched::Unreachable,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cached {
    value: u64,
    etag: Option<String>,
    fetched_at_ms: u64,
}

#[derive(Default)]
struct State {
    loaded: bool,
    values: HashMap<String, Cached>,
    /// When each source was last asked, and how that went.
    last_asked: HashMap<String, (u64, MetricState)>,
    blocked_until_ms: u64,
}

/// One per app: the lock is what keeps requests serial.
pub struct LiveMetrics {
    state: Mutex<State>,
    client: reqwest::Client,
    base: String,
    cache_path: Option<PathBuf>,
}

fn build_client(https_only: bool) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("runlith-desktop/", env!("CARGO_PKG_VERSION")))
        .timeout(REQUEST_TIMEOUT)
        .https_only(https_only)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let same_host = attempt.url().host_str() == Some(API_HOST);
            if same_host && attempt.previous().len() < 3 {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
}

impl LiveMetrics {
    pub fn new(cache_path: PathBuf) -> reqwest::Result<Self> {
        Ok(Self {
            state: Mutex::new(State::default()),
            client: build_client(true)?,
            base: API_BASE.to_owned(),
            cache_path: Some(cache_path),
        })
    }

    /// The last known value and the state of the most recent attempt. Asks
    /// GitHub only when this source is due and no rate limit is in force.
    pub async fn read(
        &self,
        source: &MetricSource,
        every: RefreshSeconds,
        now_ms: u64,
    ) -> MetricDto {
        let key = format!("{}:{}", source.kind.as_wire(), source.repo.as_str());
        let mut state = self.state.lock().await;
        self.load_once(&mut state, now_ms);

        let answer = |state: &State, status: MetricState, retry_at_ms: Option<u64>| {
            let cached = state.values.get(&key);
            MetricDto {
                value: cached.map(|c| c.value),
                fetched_at_ms: cached.map(|c| c.fetched_at_ms),
                state: status,
                retry_at_ms,
            }
        };
        if state.blocked_until_ms > now_ms {
            let until = state.blocked_until_ms;
            return answer(&state, MetricState::RateLimited, Some(until));
        }
        let interval_ms = u64::from(every.get()) * 1000;
        if let Some((asked_at, status)) = state.last_asked.get(&key).copied() {
            if now_ms.saturating_sub(asked_at) + INTERVAL_SLACK_MS < interval_ms {
                return answer(&state, status, Some(asked_at + interval_ms));
            }
        }

        let etag = state.values.get(&key).and_then(|c| c.etag.clone());
        let fetched = fetch(
            &self.client,
            &self.base,
            source,
            etag.as_deref(),
            now_ms / 1000,
        )
        .await;
        let status = match fetched {
            Fetched::Value { value, etag } => {
                let cached = Cached {
                    value,
                    etag,
                    fetched_at_ms: now_ms,
                };
                state.values.insert(key.clone(), cached);
                self.persist(&state);
                MetricState::Fresh
            }
            Fetched::NotModified => {
                if let Some(cached) = state.values.get_mut(&key) {
                    cached.fetched_at_ms = now_ms;
                }
                self.persist(&state);
                MetricState::Fresh
            }
            Fetched::NotFound => {
                state.values.remove(&key);
                self.persist(&state);
                MetricState::NotFound
            }
            Fetched::RateLimited { wait_seconds } => {
                state.blocked_until_ms = now_ms + wait_seconds * 1000;
                let until = state.blocked_until_ms;
                return answer(&state, MetricState::RateLimited, Some(until));
            }
            Fetched::Unreachable => MetricState::Offline,
        };
        state.last_asked.insert(key.clone(), (now_ms, status));
        answer(&state, status, Some(now_ms + interval_ms))
    }

    fn load_once(&self, state: &mut State, now_ms: u64) {
        if std::mem::replace(&mut state.loaded, true) {
            return;
        }
        let Some(path) = &self.cache_path else { return };
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        // A broken cache only costs a request.
        if let Ok(values) = serde_json::from_str::<HashMap<String, Cached>>(&text) {
            // The interval holds across restarts. A time from the future only
            // means the clock moved, so it does not count as having asked.
            state.last_asked = values
                .iter()
                .filter(|(_, cached)| cached.fetched_at_ms <= now_ms)
                .map(|(key, cached)| (key.clone(), (cached.fetched_at_ms, MetricState::Fresh)))
                .collect();
            state.values = values;
        }
    }

    fn persist(&self, state: &State) {
        let Some(path) = &self.cache_path else { return };
        let Ok(bytes) = serde_json::to_vec(&state.values) else {
            return;
        };
        if let Err(error) = write_atomic(path, &bytes) {
            eprintln!("runlith: could not save the widget value cache: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn source(kind: SourceType, repo: &str) -> MetricSource {
        MetricSource {
            kind,
            repo: repo.to_owned().try_into().expect("repo"),
        }
    }

    fn every(seconds: u32) -> RefreshSeconds {
        seconds.try_into().expect("refresh")
    }

    /// Answers each request with the next canned response and records requests.
    fn server(
        responses: Vec<String>,
    ) -> (String, Arc<AtomicUsize>, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let hits = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (hits_out, seen_out) = (hits.clone(), seen.clone());
        std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).expect("read");
                seen.lock()
                    .expect("seen")
                    .push(String::from_utf8_lossy(&buffer[..read]).into_owned());
                hits.fetch_add(1, Ordering::SeqCst);
                stream.write_all(response.as_bytes()).expect("write");
            }
        });
        (base, hits_out, seen_out)
    }

    fn http(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\n{headers}content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn metrics(base: String) -> LiveMetrics {
        LiveMetrics {
            state: Mutex::new(State::default()),
            client: build_client(false).expect("client"),
            base,
            cache_path: None,
        }
    }

    #[test]
    fn the_pr_count_comes_from_the_last_page_not_from_per_page() {
        let link = r#"<https://api.github.com/repositories/1/pulls?state=open&per_page=1&page=2>; rel="next", <https://api.github.com/repositories/1/pulls?state=open&per_page=1&page=53>; rel="last""#;
        assert_eq!(open_prs_from(Some(link), b"[]"), Some(53));
        // No Link header: zero or one, by the body.
        assert_eq!(open_prs_from(None, b"[]"), Some(0));
        assert_eq!(open_prs_from(None, br#"[{"id":1}]"#), Some(1));
        assert_eq!(open_prs_from(None, b"not json"), None);
    }

    #[test]
    fn stars_are_read_from_the_repository_document() {
        assert_eq!(stars_from(br#"{"stargazers_count":46831}"#), Some(46831));
        assert_eq!(stars_from(br#"{"name":"x"}"#), None);
    }

    #[test]
    fn urls_are_built_only_from_the_validated_source() {
        assert_eq!(
            url_for(API_BASE, &source(SourceType::GithubRepoStars, "cli/cli")),
            "https://api.github.com/repos/cli/cli"
        );
        assert_eq!(
            url_for(API_BASE, &source(SourceType::GithubRepoOpenPrs, "cli/cli")),
            "https://api.github.com/repos/cli/cli/pulls?state=open&per_page=1"
        );
    }

    #[test]
    fn production_client_refuses_plain_http() {
        let client = build_client(true).expect("client");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let result = runtime.block_on(client.get("http://example.com/").send());
        assert!(result.is_err(), "https_only must refuse http");
    }

    #[tokio::test]
    async fn a_source_is_not_asked_again_inside_its_interval() {
        let (base, hits, _) = server(vec![
            http("200 OK", "etag: \"a\"\r\n", r#"{"stargazers_count":10}"#),
            http("200 OK", "etag: \"b\"\r\n", r#"{"stargazers_count":11}"#),
        ]);
        let metrics = metrics(base);
        let stars = source(SourceType::GithubRepoStars, "cli/cli");
        let first = metrics.read(&stars, every(300), 1_000_000).await;
        assert_eq!((first.value, first.state), (Some(10), MetricState::Fresh));
        let early = metrics.read(&stars, every(300), 1_100_000).await;
        assert_eq!(early.value, Some(10));
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "no request inside the interval"
        );
        let due = metrics.read(&stars, every(300), 1_300_000).await;
        assert_eq!(due.value, Some(11));
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_unchanged_value_uses_the_etag_and_keeps_the_number() {
        let (base, _, seen) = server(vec![
            http("200 OK", "etag: \"abc\"\r\n", r#"{"stargazers_count":7}"#),
            http("304 Not Modified", "", ""),
        ]);
        let metrics = metrics(base);
        let stars = source(SourceType::GithubRepoStars, "cli/cli");
        metrics.read(&stars, every(300), 1_000_000).await;
        let again = metrics.read(&stars, every(300), 1_400_000).await;
        assert_eq!((again.value, again.state), (Some(7), MetricState::Fresh));
        assert_eq!(again.fetched_at_ms, Some(1_400_000));
        let requests = seen.lock().expect("seen");
        assert!(requests[1]
            .to_lowercase()
            .contains("if-none-match: \"abc\""));
    }

    #[tokio::test]
    async fn a_rate_limit_stops_every_source_until_it_resets() {
        let (base, hits, _) = server(vec![http(
            "403 Forbidden",
            "x-ratelimit-remaining: 0\r\nx-ratelimit-reset: 2000\r\n",
            "{}",
        )]);
        let metrics = metrics(base);
        let stars = source(SourceType::GithubRepoStars, "cli/cli");
        let limited = metrics.read(&stars, every(300), 1_000_000).await;
        assert_eq!(limited.state, MetricState::RateLimited);
        assert_eq!(limited.retry_at_ms, Some(2_000_000));
        let prs = source(SourceType::GithubRepoOpenPrs, "cli/cli");
        let other = metrics.read(&prs, every(300), 1_500_000).await;
        assert_eq!(other.state, MetricState::RateLimited);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the limit stops all sources"
        );
    }

    #[tokio::test]
    async fn a_missing_repository_says_so_and_drops_the_old_number() {
        let (base, _, _) = server(vec![
            http("200 OK", "", r#"{"stargazers_count":5}"#),
            http("404 Not Found", "", "{}"),
        ]);
        let metrics = metrics(base);
        let stars = source(SourceType::GithubRepoStars, "cli/gone");
        metrics.read(&stars, every(300), 1_000_000).await;
        let gone = metrics.read(&stars, every(300), 1_400_000).await;
        assert_eq!((gone.value, gone.state), (None, MetricState::NotFound));
    }

    #[tokio::test]
    async fn without_a_network_the_last_value_stays_and_the_state_says_offline() {
        let (base, _, _) = server(vec![http("200 OK", "", r#"{"stargazers_count":9}"#)]);
        let metrics = metrics(base);
        let stars = source(SourceType::GithubRepoStars, "cli/cli");
        let first = metrics.read(&stars, every(300), 1_000_000).await;
        assert_eq!(first.value, Some(9));
        let offline = metrics.read(&stars, every(300), 1_400_000).await;
        assert_eq!(offline.state, MetricState::Offline);
        assert_eq!(offline.value, Some(9));
        assert_eq!(offline.fetched_at_ms, Some(1_000_000));
    }

    #[tokio::test]
    async fn garbage_from_the_server_is_not_a_number() {
        let (base, _, _) = server(vec![http("200 OK", "", "<html>oops</html>")]);
        let metrics = metrics(base);
        let stars = source(SourceType::GithubRepoStars, "cli/cli");
        let read = metrics.read(&stars, every(300), 1_000_000).await;
        assert_eq!((read.value, read.state), (None, MetricState::Offline));
    }

    #[tokio::test]
    async fn values_survive_a_restart_through_the_cache_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("metric-cache.json");
        let (base, _, _) = server(vec![http("200 OK", "", r#"{"stargazers_count":3}"#)]);
        let first = LiveMetrics {
            cache_path: Some(path.clone()),
            ..metrics(base)
        };
        let stars = source(SourceType::GithubRepoStars, "cli/cli");
        first.read(&stars, every(300), 1_000_000).await;
        // A new process with no network still shows the last value.
        let second = LiveMetrics {
            cache_path: Some(path),
            ..metrics("http://127.0.0.1:9".to_owned())
        };
        let read = second.read(&stars, every(300), 1_400_000).await;
        assert_eq!((read.value, read.state), (Some(3), MetricState::Offline));
    }

    #[tokio::test]
    async fn a_restart_inside_the_interval_does_not_ask_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("metric-cache.json");
        let (base, hits, _) = server(vec![
            http("200 OK", "", r#"{"stargazers_count":3}"#),
            http("200 OK", "", r#"{"stargazers_count":4}"#),
        ]);
        let stars = source(SourceType::GithubRepoStars, "cli/cli");
        let first = LiveMetrics {
            cache_path: Some(path.clone()),
            ..metrics(base.clone())
        };
        first.read(&stars, every(300), 1_000_000).await;

        let restarted = LiveMetrics {
            cache_path: Some(path.clone()),
            ..metrics(base.clone())
        };
        let read = restarted.read(&stars, every(300), 1_100_000).await;
        assert_eq!((read.value, read.state), (Some(3), MetricState::Fresh));
        assert_eq!(read.retry_at_ms, Some(1_300_000));
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "a restart is not a reason to ask"
        );

        let due = restarted.read(&stars, every(300), 1_300_000).await;
        assert_eq!(due.value, Some(4));
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_cached_time_from_the_future_does_not_block_requests() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("metric-cache.json");
        let (base, hits, _) = server(vec![
            http("200 OK", "", r#"{"stargazers_count":3}"#),
            http("200 OK", "", r#"{"stargazers_count":4}"#),
        ]);
        let stars = source(SourceType::GithubRepoStars, "cli/cli");
        let first = LiveMetrics {
            cache_path: Some(path.clone()),
            ..metrics(base.clone())
        };
        first.read(&stars, every(300), 9_000_000).await;
        // The clock was set back between runs.
        let restarted = LiveMetrics {
            cache_path: Some(path),
            ..metrics(base)
        };
        let read = restarted.read(&stars, every(300), 1_000_000).await;
        assert_eq!(read.value, Some(4));
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }
}
