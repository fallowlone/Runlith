import assert from "node:assert/strict";
import { test } from "node:test";
import { expiryNote, isExpired, waitingFor } from "../src/lib/decisionTime.ts";

const NOW = 1_800_000_000_000;
const MIN = 60_000;

test("waitingFor rounds down through minutes, hours and days", () => {
  assert.equal(waitingFor(NOW - 30_000, NOW), "ждёт меньше минуты");
  assert.equal(waitingFor(NOW - 14 * MIN, NOW), "ждёт 14 мин");
  assert.equal(waitingFor(NOW - 119 * MIN, NOW), "ждёт 1 ч");
  assert.equal(waitingFor(NOW - 49 * 60 * MIN, NOW), "ждёт 2 дн");
});

test("waitingFor does not show a negative age when clocks disagree", () => {
  assert.equal(waitingFor(NOW + 5 * MIN, NOW), "ждёт меньше минуты");
});

test("expiryNote separates no deadline, a running deadline and a passed one", () => {
  assert.equal(expiryNote(null, NOW), "бессрочно");
  assert.equal(expiryNote(NOW + 12 * MIN, NOW), "срок 12 мин");
  assert.equal(expiryNote(NOW, NOW), "срок истёк");
  assert.equal(expiryNote(NOW - MIN, NOW), "срок истёк");
});

test("isExpired never treats a missing deadline as expired", () => {
  assert.equal(isExpired(null, NOW), false);
  assert.equal(isExpired(NOW - 1, NOW), true);
});
