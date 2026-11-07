import concurrent.futures
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import time
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parent
COMMIT = "ff6aec96948b70d94983af2641a6b67c94faeff5"
TAG = "8b9fa496bbf2c47aebd62e85a080b9a522a455b5"
HOSTS = {"learn.chatgpt.com", "developers.openai.com", "api.github.com", "raw.githubusercontent.com"}
SPECS = [
    ("tag-ref", "https://api.github.com/repos/openai/codex/git/ref/tags/rust-v0.159.2", []),
    ("tag-object", f"https://api.github.com/repos/openai/codex/git/tags/{TAG}", []),
    ("doc-auth", "https://learn.chatgpt.com/docs/auth", ["Credential storage", "cli_auth_credentials_store", "refreshes"]),
    ("doc-exec", "https://learn.chatgpt.com/docs/non-interactive-mode", ["--json", "--ephemeral", "authentication"]),
    ("doc-config", "https://learn.chatgpt.com/docs/config-file/config-reference", ["allow_login_shell", "cli_auth_credentials_store", "sandbox_read_only", "mcp_servers"]),
]
SOURCE_RANGES = {
    "codex-rs/exec/src/cli.rs": [(10, 175)],
    "codex-rs/exec/src/exec_events.rs": [(8, 141)],
    "codex-rs/exec/src/event_processor_with_jsonl_output.rs": [(104, 128), (447, 457), (508, 564)],
    "codex-rs/exec/src/lib.rs": [(1126, 1132), (1224, 1278), (1293, 1326)],
    "codex-rs/core/src/tools/spec_plan.rs": [(150, 179), (349, 356), (621, 650), (1079, 1116), (1132, 1155), (1269, 1315), (1477, 1504)],
    "codex-rs/features/src/lib.rs": [(611, 613), (962, 1015), (1190, 1195), (1326, 1331), (1362, 1367), (1440, 1457)],
    "codex-rs/ext/extension-api/src/tool_policy.rs": [(6, 44)],
    "codex-rs/core/src/session/session.rs": [(983, 992)],
    "codex-rs/core/src/tools/registry.rs": [(339, 386)],
    "codex-rs/config/src/config_toml.rs": [(663, 673)],
    "codex-rs/core/src/config/managed_features.rs": [(153, 167)],
    "codex-rs/core/src/agents_md.rs": [(58, 87)],
    "codex-rs/config/src/mcp_types.rs": [(244, 256), (292, 298)],
    "codex-rs/core/src/tools/hosted_spec.rs": [(14, 20)],
    "codex-rs/ext/web-search/src/extension.rs": [(42, 54)],
    "codex-rs/codex-api/src/sse/responses.rs": [(1424, 1433)],
}
SPECS.extend((
    path.replace("/", "__").removesuffix(".rs"),
    f"https://raw.githubusercontent.com/openai/codex/{COMMIT}/{path}",
    ranges,
) for path, ranges in SOURCE_RANGES.items())


class Text(HTMLParser):
    def __init__(self):
        super().__init__()
        self.parts = []
        self.skip = 0

    def handle_starttag(self, tag, attrs):
        if tag in ("script", "style"):
            self.skip += 1

    def handle_endtag(self, tag):
        if tag in ("script", "style") and self.skip:
            self.skip -= 1

    def handle_data(self, data):
        if not self.skip and data.strip():
            self.parts.append(data.strip())


class Redirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        parsed = urllib.parse.urlparse(newurl)
        if parsed.scheme != "https" or parsed.hostname not in HOSTS:
            raise ValueError("Unexpected acquisition redirect")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def fetch(spec):
    name, url, selectors = spec
    parsed = urllib.parse.urlparse(url)
    assert parsed.scheme == "https" and parsed.hostname in HOSTS
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), Redirect())
    request = urllib.request.Request(url, headers={"User-Agent": "Runlith-TASK-0156-public-evidence"})
    with opener.open(request, timeout=30) as response:
        raw = response.read(8 * 1024 * 1024 + 1)
        assert len(raw) <= 8 * 1024 * 1024
        status, final_url = response.status, response.url
    text = raw.decode("utf-8")
    if name.startswith("doc-"):
        parser = Text()
        parser.feed(text)
        normalized = "\n".join(parser.parts)
        blocks = []
        for needle in selectors:
            start = normalized.lower().find(needle.lower())
            assert start >= 0, (name, needle)
            blocks.append(f"Selector: {needle}\n{normalized[max(0, start-150):start+1300]}")
        saved = "\n\n".join(blocks) + "\n"
        scope = "Mutable official documentation observed at acquisition date; NOT proof of 0.159.2 semantics. Selected normalized HTML text."
    elif selectors:
        lines = text.splitlines()
        assert all(1 <= first <= last <= len(lines) for first, last in selectors)
        saved = "\n\n".join(
            f"Lines {first}-{last}\n" + "\n".join(f"{i}: {lines[i-1]}" for i in range(first, last+1))
            for first, last in selectors
        ) + "\n"
        scope = f"Official openai/codex rust-v0.159.2 source at {COMMIT}; numbered excerpts for evidence only, no imported implementation."
    else:
        data = json.loads(text)
        if name == "tag-ref":
            assert data["object"]["sha"] == TAG and data["object"]["type"] == "tag"
        if name == "tag-object":
            assert data["object"]["sha"] == COMMIT and data["object"]["type"] == "commit"
        saved, scope = text, "Official GitHub version provenance; original response retained."
    path = ROOT / "sources" / f"{name}.txt"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(saved)
    return {
        "id": name, "url": url, "finalUrl": final_url, "httpStatus": status,
        "observedAtUtc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "tag": "rust-v0.159.2" if not name.startswith("doc-") else None,
        "commit": COMMIT if not name.startswith("doc-") else None,
        "scope": scope, "selectors": selectors, "responseBytes": len(raw),
        "responseSha256": hashlib.sha256(raw).hexdigest(),
        "savedPath": path.relative_to(ROOT).as_posix(),
        "savedSha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }


def capture():
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        records = list(pool.map(fetch, SPECS))
    (ROOT / "sources-manifest.json").write_text(json.dumps(records, indent=2) + "\n")
    print(f"PASS: {len(records)} public primary-source acquisitions, tag/SHA verified; proxies and credentials not inherited")


if __name__ == "__main__":
    capture()
