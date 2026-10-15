import assert from "node:assert/strict";
import test from "node:test";

import {
  documentationViolations,
  frontendSourceViolations,
  frontendViolations,
  rustLayerViolations,
  rustLayerSourceViolations,
  rustSqlSourceViolations,
  rustSqlViolations,
} from "./architecture-rules.mjs";

test("named workflow repositories own SQL but HTTP and services cannot", () => {
  const source = 'sqlx::query("SELECT 1");';

  for (const filename of [
    "src/conversations/group_repository.rs",
    "src/media/repository.rs",
  ]) {
    assert.deepEqual(rustSqlSourceViolations(filename, source), []);
  }
  for (const filename of [
    "src/conversations/group_http.rs",
    "src/media/service.rs",
    "src/bin/worker.rs",
  ]) {
    assert.equal(rustSqlSourceViolations(filename, source).length, 1);
  }
});

test("media and group workflows cannot import named HTTP adapters", () => {
  for (const filename of [
    "src/media/service.rs",
    "src/conversations/group_repository.rs",
  ]) {
    assert.equal(
      rustLayerSourceViolations(filename, "use super::group_http::create;")
        .length,
      1,
    );
    assert.equal(
      rustLayerSourceViolations(filename, "use crate::media::{http, model};")
        .length,
      1,
    );
    assert.deepEqual(
      rustLayerSourceViolations(filename, "use super::repository;"),
      [],
    );
  }
  assert.deepEqual(
    rustLayerSourceViolations(
      "src/conversations/group_http.rs",
      "use super::http::actor;",
    ),
    [],
  );
});

test("new feature workflows cannot bypass the Rust HTTP boundary", () => {
  const source = "use super::http::register;";

  for (const filename of [
    "src/notifications/service.rs",
    "src/notifications/job_repository.rs",
    "src/example_feature/workflow.rs",
  ]) {
    assert.equal(rustLayerSourceViolations(filename, source).length, 1);
  }

  for (const filename of [
    "src/notifications/http.rs",
    "src/example_feature/account_http.rs",
    "src/bin/server.rs",
  ]) {
    assert.deepEqual(rustLayerSourceViolations(filename, source), []);
  }
});

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

const fetchCalls = [
  "fetch('/api/v1/auth/me')",
  "window.fetch('/api/v1/auth/me')",
  "globalThis.fetch('/api/v1/auth/me')",
  "self.fetch('/api/v1/auth/me')",
  "window['fetch']('/api/v1/auth/me')",
  "globalThis[`fetch`]('/api/v1/auth/me')",
  "fetch?.('/api/v1/auth/me')",
  "window?.fetch?.('/api/v1/auth/me')",
  "(fetch)('/api/v1/auth/me')",
  "(globalThis as Window).fetch('/api/v1/auth/me')",
];

for (const call of fetchCalls) {
  test(`frontend rejects raw HTTP outside adapters: ${call}`, () => {
    const violations = frontendSourceViolations(
      "web/src/lib/features/chat/sync.svelte.ts",
      `${call};`,
    );

    assert.equal(violations.length, 1);
    assert.match(violations[0], /HTTP calls belong in a feature API adapter/);
  });
}

test("feature APIs and shared transport may own raw fetch", () => {
  const adapterPaths = [
    "web/src/lib/features/auth/api.ts",
    "web/src/lib/features/chat/api.ts",
    "web/src/lib/api-client/client.ts",
  ];

  for (const filename of adapterPaths) {
    for (const call of fetchCalls) {
      const violations = frontendSourceViolations(filename, `${call};`);

      assert.deepEqual(violations, [], `${filename}: ${call}`);
    }
  }
});

test("routes and nested component files cannot bypass the HTTP boundary", () => {
  const filenames = [
    "web/src/routes/app/+page.ts",
    "web/src/lib/features/chat/components/api.ts",
    "web/src/lib/helpers.ts",
  ];

  for (const filename of filenames) {
    const violations = frontendSourceViolations(filename, "fetch('/api');");

    assert.equal(violations.length, 1, filename);
  }
});

const componentSources = [
  '<script lang="ts">fetch("/api");</script>',
  '<script module lang="ts">globalThis.fetch("/api");</script>',
  '<button onclick={() => fetch("/api")}>Load</button>',
  '<button onclick={() => window["fetch"]("/api")}>Load</button>',
  '{#await globalThis.fetch("/api") then response}<p>Loaded</p>{/await}',
  '<svelte:options customElement={{ tag: "chat-example", extend: (Base) => class extends Base { constructor() { super(); fetch("/api"); } } }} />',
];

for (const source of componentSources) {
  test(`Svelte HTTP checks include scripts and template expressions: ${source}`, () => {
    const violations = frontendSourceViolations(
      "web/src/lib/features/chat/Example.svelte",
      source,
    );

    assert.equal(violations.length, 1);
  });
}

test("HTTP checks ignore comments, strings and unrelated object methods", () => {
  const source = `
    // fetch('/api') and window.fetch('/api') are documentation here.
    const example = "globalThis.fetch('/api')";
    const fetchReference = globalThis.fetch;
    cache.fetch();
    window.fetchStatus();
  `;
  const component =
    '<p>fetch("/api")</p><button onclick={() => cache.fetch()}>Load</button>';

  assert.deepEqual(
    frontendSourceViolations("web/src/lib/features/chat/example.ts", source),
    [],
  );
  assert.deepEqual(
    frontendSourceViolations(
      "web/src/lib/features/chat/Example.svelte",
      component,
    ),
    [],
  );
});

test("typed HTTP method checks retain the same adapter boundary", () => {
  const source = "api.GET('/api/v1/auth/me');";
  const violations = frontendSourceViolations(
    "web/src/lib/features/chat/Example.svelte",
    `<script lang="ts">${source}</script>`,
  );

  assert.equal(violations.length, 1);
  assert.deepEqual(
    frontendSourceViolations("web/src/lib/features/chat/api.ts", source),
    [],
  );
});
