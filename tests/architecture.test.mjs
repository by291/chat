import assert from "node:assert/strict";
import test from "node:test";

import {
  documentationViolations,
  frontendViolations,
  rustLayerViolations,
  rustSqlViolations,
} from "../scripts/architecture-rules.mjs";

test("Rust keeps business SQL in concrete repositories", async () => {
  const violations = await rustSqlViolations();

  assert.deepEqual(violations, [], violations.join("\n"));
});

test("Rust application and persistence modules remain independent of HTTP adapters", async () => {
  const violations = await rustLayerViolations();

  assert.deepEqual(violations, [], violations.join("\n"));
});

test("frontend keeps HTTP in feature adapters and shared code independent", async () => {
  const violations = await frontendViolations();

  assert.deepEqual(violations, [], violations.join("\n"));
});

test("repository documentation follows the English language policy", async () => {
  const violations = await documentationViolations();

  assert.deepEqual(violations, [], violations.join("\n"));
});
