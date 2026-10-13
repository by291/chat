/** Shared Windows/Linux checks. Middleware and subprocess cleanup belong to this run. */
import { readFile } from "node:fs/promises";
import path from "node:path";

import {
  repositoryRoot,
  prepareTestInfrastructure,
  runCommand,
} from "../web/tests/integration/runtime.ts";

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
const infrastructure = await prepareTestInfrastructure();
const env = {
  ...process.env,
  ...infrastructure.values,
  CHAT_TEST_INFRASTRUCTURE_ENV: infrastructure.envFile,
};
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
    "Architecture boundaries",
    () => command(process.execPath, ["--test", "tests/architecture.test.mjs"]),
  ],
  ["Rust format", () => command("cargo", ["fmt", "--all", "--", "--check"])],
  [
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
    "Rust tests and real media",
    () => command("cargo", ["test", "--", "--include-ignored"]),
  ],
  ["SvelteKit sync", () => tool("@sveltejs/kit", "svelte-kit", ["sync"])],
  [
    "Svelte types",
    () =>
      tool("svelte-check", "svelte-check", ["--tsconfig", "./tsconfig.json"]),
  ],
  [
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
      ]),
  ],
  [
    "API schema",
    () => command(process.execPath, ["scripts/generate-api.mjs", "--check"]),
  ],
  ["Frontend unit tests", () => tool("vitest", "vitest", ["run"])],
  ["Static build", () => tool("vite", "vite", ["build"])],
];
if (process.argv.includes("--integration")) {
  steps.push(
    [
      "SDK and recovery integration",
      () =>
        tool("vitest", "vitest", [
          "run",
          "--config",
          "vitest.integration.config.ts",
        ]),
    ],
    [
      "Browser scenarios",
      () => tool("@playwright/test", "playwright", ["test"]),
    ],
  );
}

const failures = [];
try {
  for (const [name, operation] of steps) {
    console.log(`Checking ${name}`);
    await operation();
  }
} catch (error) {
  failures.push(error);
} finally {
  try {
    await infrastructure.cleanup();
  } catch (error) {
    failures.push(error);
  }
}
if (failures.length)
  throw new AggregateError(failures, "Repository checks failed");
