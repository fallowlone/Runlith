import assert from "node:assert/strict";
import { test } from "node:test";
import { failureText, isRepoName, nextReadDelay } from "../src/lib/widgetText.ts";

test("isRepoName takes owner/name and nothing else, as RepoName does in widget-spec", () => {
  for (const good of ["cli/cli", "fallowlone/runlith", "a-b/c.d_e-f", "a--b/c"]) assert.equal(isRepoName(good), true, good);
  const tooLongOwner = `${"a".repeat(40)}/x`;
  const tooLongName = `a/${"x".repeat(101)}`;
  for (const bad of ["", "cli", "/cli", "cli/", "-a/b", "a-/b", "a/.", "a/..", "a/b/c", "a b/c", "http://example.com", "a/b?c", tooLongOwner, tooLongName]) {
    assert.equal(isRepoName(bad), false, bad);
  }
});

test("each credential refusal says, in Russian, which command fixes it", () => {
  const steps: Array<[string, string]> = [
    ["provider_credential_vault_absent: no operator vault exists", "agentctl vault init"],
    ["provider_credential_vault_unavailable: cannot be read", "agentctl vault status"],
    ["provider_credential_vault_locked: locked", "agentctl vault unlock"],
    ["provider_credential_missing: none stored", "claude setup-token"],
    ["provider_credential_expired: past its lifetime", "claude setup-token"],
    ["provider_credential_rejected: refused", "claude setup-token"],
  ];
  for (const [detail, command] of steps) {
    const text = failureText("not_authenticated", detail);
    assert.ok(text.includes(command.replaceAll(" ", "\u00A0")), `${text}: the command must not wrap`);
    assert.ok(!text.includes("provider_credential"), text);
  }
});

test("the reason is found wherever agentd puts it in the detail", () => {
  // Verbatim from a real run: the real CLI, under the real sandbox, refused with a 401.
  const real = "claude credential_rejected: provider_credential_rejected: the provider refused the presented credential (api_error_status 401)";
  assert.match(failureText("not_authenticated", real), /отклонил сохранённый токен/);
});

test("a sign-in refusal without a vault reason points at the CLI", () => {
  assert.match(failureText("not_authenticated", "not logged in"), /claude/);
});

test("provider prose in a failure detail never reaches the screen", () => {
  for (const code of ["provider_missing", "provider_incompatible", "provider_unavailable", "timed_out", "bad_output", "failed", "cancelled", null]) {
    assert.ok(!failureText(code, "stderr-excerpt from the model").includes("stderr-excerpt"), String(code));
  }
});

test("the next read waits for the moment the host allows, so a value is never older than its interval", () => {
  const now = 1_000_000;
  assert.equal(nextReadDelay(now + 1_000, 300, now), 1_000, "restart just before the interval ends");
  assert.equal(nextReadDelay(now + 720_000, 300, now), 720_000, "a rate limit outlasts the interval");
  assert.equal(nextReadDelay(null, 300, now), 300_000, "no hint from the host");
  assert.equal(nextReadDelay(now - 5_000, 300, now), 1_000, "already due, but never a busy loop");
});
