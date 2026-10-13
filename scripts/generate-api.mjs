/** Generate deterministic OpenAPI and TypeScript from the Rust-owned contract. */
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const web = join(root, "web");
const scratch = mkdtempSync(join(tmpdir(), "chat-api-"));

try {
  const schema = execFileSync(
    "cargo",
    [
      "run",
      "--quiet",
      "--manifest-path",
      join(root, "Cargo.toml"),
      "--bin",
      "export_openapi",
    ],
    { cwd: root, encoding: "utf8" },
  ).replaceAll("\r\n", "\n");
  const input = join(scratch, "openapi.json");
  const output = join(scratch, "schema.d.ts");
  writeFileSync(input, schema);

  execFileSync(
    process.execPath,
    [
      join(web, "node_modules/openapi-typescript/bin/cli.js"),
      input,
      "--output",
      output,
    ],
    { cwd: web, stdio: "pipe" },
  );
  const generated = readFileSync(output, "utf8").replaceAll("\r\n", "\n");
  const destination = join(web, "src/lib/api-client/schema.d.ts");
  const openapiDestination = join(web, "openapi.json");

  if (process.argv.includes("--check")) {
    // Both exported JSON and generated TS are checked: descriptions can otherwise
    // drift in the JSON even when they do not affect TypeScript declarations.
    if (
      readFileSync(destination, "utf8") !== generated ||
      readFileSync(openapiDestination, "utf8") !== schema
    ) {
      throw new Error("API schema differs; run pnpm api:generate");
    }
  } else {
    writeFileSync(destination, generated);
    writeFileSync(openapiDestination, schema);
  }
} finally {
  // This unique temporary directory is the only cleanup target.
  rmSync(scratch, { recursive: true, force: true });
}
