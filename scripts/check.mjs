/** Shared Windows/Linux checks. Middleware and subprocess cleanup belong to this run. */
import { readFile } from "node:fs/promises";
import path from "node:path";
import { parseArgs } from "node:util";

import {
  repositoryRoot,
  prepareTestInfrastructure,
  runCommand,
} from "../tests/infrastructure.mts";

const { values } = parseArgs({
  options: {
    scope: { type: "string", default: "all" },
    integration: { type: "boolean", default: false },
  },
});
const scope = values.scope;
if (!["all", "backend", "frontend", "integration"].includes(scope)) {
  throw new Error("Expected --scope all, backend, frontend or integration");
}
if (values.integration && scope !== "all") {
  throw new Error(
    "Use --integration with the default scope, or --scope integration",
  );
}

const web = path.join(repositoryRoot, "web");
const controller = new AbortController();
const cancel = () => {
  controller.abort();
  console.log(
    "Cancellation requested; waiting for the current phase to clean up.",
  );
};
process.once("SIGINT", cancel);
process.once("SIGTERM", cancel);
const env = { ...process.env };
const command = async (name, args, cwd = repositoryRoot) => {
  controller.signal.throwIfAborted();
  // Killing only Cargo/Vitest/Playwright can orphan their fixture processes on Windows.
  // Let the current owner drain, then stop before another phase acquires resources.
  const output = await runCommand(name, args, { cwd, env });
  controller.signal.throwIfAborted();
  return output;
};
const tool = async (packageName, binName, args) => {
  const directory = path.join(web, "node_modules", packageName);
  const manifest = JSON.parse(
    await readFile(path.join(directory, "package.json"), "utf8"),
  );
  const binary =
    typeof manifest.bin === "string" ? manifest.bin : manifest.bin[binName];
  return command(
    process.execPath,
    [path.join(directory, binary), ...args],
    web,
  );
};
const steps = [
  [
    "frontend",
    "Architecture boundaries",
    () => command(process.execPath, ["--test", "tests/architecture.test.mjs"]),
  ],
  [
    "backend",
    "Rust format",
    () => command("cargo", ["fmt", "--all", "--", "--check"]),
  ],
  [
    "backend",
    "Rust lint",
    () =>
      command("cargo", [
        "clippy",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
      ]),
  ],
  [
    "backend",
    "Rust tests and real media",
    () =>
      command("cargo", ["test", "--all-features", "--", "--include-ignored"]),
  ],
  [
    "frontend",
    "SvelteKit sync",
    () => tool("@sveltejs/kit", "svelte-kit", ["sync"]),
  ],
  [
    "frontend",
    "Svelte types",
    () =>
      tool("svelte-check", "svelte-check", ["--tsconfig", "./tsconfig.json"]),
  ],
  [
    "frontend",
    "Formatting",
    () =>
      tool("prettier", "prettier", [
        "--check",
        "src",
        "tests",
        "e2e",
        "*.ts",
        "*.json",
        "../scripts/*.mjs",
        "../tests/*.mjs",
        "../tests/*.{ts,mts}",
      ]),
  ],
  [
    "contract",
    "API schema",
    () => command(process.execPath, ["scripts/generate-api.mjs", "--check"]),
  ],
  ["frontend", "Frontend unit tests", () => tool("vitest", "vitest", ["run"])],
  ["frontend", "Static build", () => tool("vite", "vite", ["build"])],
  [
    "integration",
    "SDK and recovery integration",
    () =>
      tool("vitest", "vitest", [
        "run",
        "--config",
        "vitest.integration.config.ts",
      ]),
  ],
  [
    "integration",
    "Browser scenarios",
    () => tool("@playwright/test", "playwright", ["test"]),
  ],
].filter(([group]) => {
  if (scope === "all") return group !== "integration" || values.integration;
  if (group === "contract") return scope === "integration";
  return group === scope;
});

const failures = [];
let infrastructure;
try {
  if (scope !== "frontend") {
    infrastructure = await prepareTestInfrastructure();
    Object.assign(env, infrastructure.values, {
      CHAT_TEST_INFRASTRUCTURE_ENV: infrastructure.envFile,
    });
  }

  for (const [, name, operation] of steps) {
    console.log(`Checking ${name}`);
    await operation();
  }
} catch (error) {
  failures.push(error);
} finally {
  try {
    await infrastructure?.cleanup();
  } catch (error) {
    failures.push(error);
  }
}
if (failures.length)
  throw new AggregateError(failures, "Repository checks failed");
