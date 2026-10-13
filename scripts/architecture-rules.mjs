/** Source-level ownership guards supplement the compiler and behavior tests. */
import { readdir, readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const requireWeb = createRequire(path.join(root, "web/package.json"));
const ts = requireWeb("typescript");

async function filesUnder(directory) {
  const entries = await readdir(path.join(root, directory), {
    withFileTypes: true,
  });
  const groups = await Promise.all(
    entries.map(async (entry) => {
      const relative = `${directory}/${entry.name}`;
      return entry.isDirectory() ? filesUnder(relative) : [relative];
    }),
  );

  return groups.flat();
}

function importsIn(source, filename) {
  const script = filename.endsWith(".svelte")
    ? [...source.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/g)]
        .map((match) => match[1])
        .join("\n")
    : source;
  const syntax = ts.createSourceFile(
    filename,
    script,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );
  const imports = [];
  let hasHttpCall = false;

  function visit(node) {
    if (
      (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
      node.moduleSpecifier &&
      ts.isStringLiteral(node.moduleSpecifier)
    ) {
      imports.push(node.moduleSpecifier.text);
    }

    if (
      ts.isCallExpression(node) &&
      ts.isPropertyAccessExpression(node.expression) &&
      ["GET", "POST", "PATCH", "PUT", "DELETE", "HEAD", "OPTIONS"].includes(
        node.expression.name.text,
      )
    ) {
      hasHttpCall = true;
    }

    ts.forEachChild(node, visit);
  }

  visit(syntax);
  return { imports, hasHttpCall };
}

function projectImport(filename, specifier) {
  if (specifier === "#lib") return "web/src/lib/index.ts";
  if (specifier.startsWith("#lib/")) return `web/src/lib/${specifier.slice(5)}`;
  if (specifier.startsWith("."))
    return path.posix.normalize(
      path.posix.join(path.posix.dirname(filename), specifier),
    );
  return null;
}

export async function rustSqlViolations() {
  const violations = [];
  const files = (await filesUnder("src")).filter((filename) =>
    filename.endsWith(".rs"),
  );

  for (const filename of files) {
    if (
      filename.endsWith("/repository.rs") ||
      filename === "src/infrastructure/db.rs"
    )
      continue;
    const source = await readFile(path.join(root, filename), "utf8");
    if (/\bsqlx\s*::\s*(?:query(?:_as|_scalar)?|raw_sql)\s*[<(]/.test(source)) {
      violations.push(
        `${filename}: move PostgreSQL queries into the owning repository`,
      );
    }
  }

  return violations;
}

export async function rustLayerViolations() {
  const violations = [];
  const files = (await filesUnder("src")).filter(
    (filename) =>
      /^src\/(?:auth|conversations|realtime|users)\//.test(filename) &&
      filename.endsWith(".rs") &&
      !filename.endsWith("/http.rs") &&
      !filename.endsWith("/mod.rs"),
  );

  for (const filename of files) {
    const source = await readFile(path.join(root, filename), "utf8");
    const imports = [...source.matchAll(/\buse\s+([^;]+);/g)];
    const importsHttpAdapter = imports.some(
      ([, declaration]) =>
        /^(?:crate|super|self)\s*::/.test(declaration) &&
        /\bhttp\s*(?:::|[,{}])/.test(declaration),
    );
    if (importsHttpAdapter) {
      violations.push(
        `${filename}: application and persistence modules must not import HTTP adapters`,
      );
    }
  }

  return violations;
}

export async function frontendViolations() {
  const violations = [];
  const files = (await filesUnder("web/src")).filter(
    (filename) =>
      /\.(?:ts|svelte)$/.test(filename) && !filename.endsWith("schema.d.ts"),
  );

  for (const filename of files) {
    const source = await readFile(path.join(root, filename), "utf8");
    const { imports, hasHttpCall } = importsIn(source, filename);
    const isFeature = filename.startsWith("web/src/lib/features/");
    const isRoute = filename.startsWith("web/src/routes/");

    if (
      hasHttpCall &&
      !filename.startsWith("web/src/lib/api-client/") &&
      !/\/features\/[^/]+\/api\.ts$/.test(filename)
    ) {
      violations.push(
        `${filename}: typed HTTP calls belong in a feature API adapter`,
      );
    }

    for (const specifier of imports) {
      const target = projectImport(filename, specifier);
      if (!target) continue;

      if (
        !isFeature &&
        !isRoute &&
        target.startsWith("web/src/lib/features/")
      ) {
        violations.push(
          `${filename}: shared code must not import feature ${specifier}`,
        );
      }

      if (isFeature && target.startsWith("web/src/routes/")) {
        violations.push(`${filename}: features must not import routes`);
      }
    }
  }

  return violations;
}

export async function documentationViolations() {
  const files = [
    "AGENTS.md",
    "README.md",
    ...(await filesUnder("docs")).filter((filename) =>
      filename.endsWith(".md"),
    ),
  ];
  const violations = [];

  for (const filename of files) {
    const source = await readFile(path.join(root, filename), "utf8");
    if (/[\u3400-\u9fff]/u.test(source)) {
      violations.push(`${filename}: repository documentation must be English`);
    }
  }

  return violations;
}
