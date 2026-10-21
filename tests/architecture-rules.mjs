/** Source-level ownership guards supplement the compiler and behavior tests. */
import { readdir, readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const requireWeb = createRequire(path.join(root, "web/package.json"));
const ts = requireWeb("typescript");
const { parse: parseComponent } = requireWeb("svelte/compiler");

const httpMethods = new Set([
  "GET",
  "POST",
  "PATCH",
  "PUT",
  "DELETE",
  "HEAD",
  "OPTIONS",
]);
const browserGlobals = new Set(["window", "globalThis", "self"]);
// Workflow adapters are explicit: a component folder named api.ts is not a boundary.
const workflowAdapters = new Set([
  "chat/conversations",
  "chat/messages",
  "chat/groups",
  "contacts/friends",
  "contacts/requests",
  "settings/profile",
  "settings/security",
  "settings/sessions",
  "auth/recovery",
  "auth/verification",
]);
const persistenceFacades = new Set([
  "src/media/repository.rs",
  "src/notifications/job_repository.rs",
]);

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

function componentScript(source, filename) {
  const component = parseComponent(source, { filename, modern: true });
  const scripts = [component.module, component.instance]
    .filter(Boolean)
    .map(({ content }) => source.slice(content.start, content.end));
  const templateCalls = [];

  function visit(node) {
    if (!node || typeof node !== "object") return;
    if (node.type === "CallExpression") {
      templateCalls.push(`(${source.slice(node.start, node.end)});`);
      return;
    }
    for (const child of Object.values(node)) {
      if (Array.isArray(child)) child.forEach(visit);
      else visit(child);
    }
  }

  // Inline handlers, template expressions and custom-element options can perform
  // HTTP too. Inspect their calls without treating markup text as code.
  visit(component.fragment);
  visit(component.options);
  return [...scripts, ...templateCalls].join("\n;\n");
}

function unwrapExpression(expression) {
  while (
    ts.isParenthesizedExpression(expression) ||
    ts.isAsExpression(expression) ||
    ts.isTypeAssertionExpression(expression) ||
    ts.isNonNullExpression(expression) ||
    ts.isSatisfiesExpression(expression)
  ) {
    expression = expression.expression;
  }
  return expression;
}

function isHttpCall(expression) {
  const callee = unwrapExpression(expression);
  if (ts.isIdentifier(callee)) return callee.text === "fetch";

  let method;
  if (ts.isPropertyAccessExpression(callee)) {
    method = callee.name.text;
  } else if (
    ts.isElementAccessExpression(callee) &&
    (ts.isStringLiteral(callee.argumentExpression) ||
      ts.isNoSubstitutionTemplateLiteral(callee.argumentExpression))
  ) {
    method = callee.argumentExpression.text;
  } else {
    return false;
  }

  if (httpMethods.has(method)) return true;
  if (method !== "fetch") return false;

  const receiver = unwrapExpression(callee.expression);
  return ts.isIdentifier(receiver) && browserGlobals.has(receiver.text);
}

function importsIn(source, filename) {
  const script = filename.endsWith(".svelte")
    ? componentScript(source, filename)
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
  let clearsQueryClient = false;

  function visit(node) {
    if (
      (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
      node.moduleSpecifier &&
      ts.isStringLiteral(node.moduleSpecifier)
    ) {
      imports.push(node.moduleSpecifier.text);
    }

    if (ts.isCallExpression(node)) {
      if (isHttpCall(node.expression)) hasHttpCall = true;

      if (
        node.expression.kind === ts.SyntaxKind.ImportKeyword &&
        node.arguments.length === 1 &&
        ts.isStringLiteral(node.arguments[0])
      ) {
        imports.push(node.arguments[0].text);
      }

      const callee = unwrapExpression(node.expression);
      if (
        ts.isPropertyAccessExpression(callee) &&
        callee.name.text === "clear"
      ) {
        const receiver = unwrapExpression(callee.expression);
        if (
          (ts.isIdentifier(receiver) && receiver.text === "queryClient") ||
          (ts.isPropertyAccessExpression(receiver) &&
            receiver.name.text === "queryClient")
        ) {
          clearsQueryClient = true;
        }
      }
    }

    ts.forEachChild(node, visit);
  }

  visit(syntax);
  return { imports, hasHttpCall, clearsQueryClient };
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
    const source = await readFile(path.join(root, filename), "utf8");
    violations.push(...rustSqlSourceViolations(filename, source));
  }

  return violations;
}

/** Inspect persistence ownership independently of the current source tree. */
export function rustSqlSourceViolations(filename, source) {
  const repository = /(?:\/|_)repository\.rs$/.test(filename);
  if (repository || filename === "src/infrastructure/db.rs") return [];
  if (!/\bsqlx\s*::\s*(?:query(?:_as|_scalar)?|raw_sql)\s*[<(]/.test(source))
    return [];

  return [`${filename}: move PostgreSQL queries into the owning repository`];
}

/** Named workflow adapters have the same boundary as a feature's http.rs. */
export function rustLayerSourceViolations(filename, source) {
  if (
    !/^src\/[^/]+\//.test(filename) ||
    filename.startsWith("src/bin/") ||
    /(?:\/|_)http\.rs$/.test(filename) ||
    filename.endsWith("/mod.rs")
  )
    return [];

  // This is a source guard, not a Rust parser. Ignore comments so documented
  // transaction preconditions are not mistaken for application operations.
  const code = source.replace(/\/\/[^\n]*|\/\*[\s\S]*?\*\//g, "");
  const imports = [...code.matchAll(/\buse\s+([^;]+);/g)];
  const importsHttpAdapter = imports.some(
    ([, declaration]) =>
      /^(?:crate|super|self)\s*::/.test(declaration) &&
      /\b(?:\w+_)?http\s*(?:::|[,{}])/.test(declaration),
  );
  const violations = [];
  if (importsHttpAdapter) {
    violations.push(
      `${filename}: application and persistence modules must not import HTTP adapters`,
    );
  }

  if (/(?:\/|_)repository\.rs$/.test(filename)) {
    const importsWorkflow = imports.some(
      ([, declaration]) =>
        /^(?:crate|super|self)\s*::/.test(declaration) &&
        /\b(?:service|delivery|worker|lifecycle)\s*(?:::|[,{}])/.test(
          declaration,
        ),
    );
    // Existing public facades export worker operations for external consumers.
    // They may re-export names, but must never grow executable persistence.
    const compatibilityExports =
      persistenceFacades.has(filename) &&
      code.replace(/\bpub\s+use\s+[^;]+;/g, "").trim() === "";
    if (importsWorkflow && !compatibilityExports) {
      violations.push(
        `${filename}: repositories must not depend on application workflows`,
      );
    }
    // begin_with is the documented isolation-level constructor: it returns the
    // transaction to its caller. Ordinary transaction lifetimes belong to services.
    if (/\.\s*(?:begin|commit|rollback)\s*\(/.test(code)) {
      violations.push(
        `${filename}: application workflows own transaction lifetimes`,
      );
    }
  }

  return violations;
}

export async function rustLayerViolations() {
  const violations = [];
  const files = (await filesUnder("src")).filter((filename) =>
    filename.endsWith(".rs"),
  );

  for (const filename of files) {
    const source = await readFile(path.join(root, filename), "utf8");
    violations.push(...rustLayerSourceViolations(filename, source));
  }

  return violations;
}

/** Inspect one source file so boundary regressions can use isolated source fixtures. */
export function frontendSourceViolations(filename, source) {
  const violations = [];
  const { imports, hasHttpCall, clearsQueryClient } = importsIn(
    source,
    filename,
  );
  const isFeature = filename.startsWith("web/src/lib/features/");
  const isRoute = filename.startsWith("web/src/routes/");
  const workflow = filename.match(
    /^web\/src\/lib\/features\/(.+)\/api\.ts$/,
  )?.[1];
  const isAdapter =
    workflow !== undefined &&
    (!workflow.includes("/") || workflowAdapters.has(workflow));

  if (
    hasHttpCall &&
    !filename.startsWith("web/src/lib/api-client/") &&
    !isAdapter
  ) {
    violations.push(`${filename}: HTTP calls belong in a feature API adapter`);
  }

  if (
    isFeature &&
    clearsQueryClient &&
    !filename.startsWith("web/src/lib/features/workspace/")
  ) {
    violations.push(
      `${filename}: only the authenticated workspace owns whole-session cache clearing`,
    );
  }

  for (const specifier of imports) {
    const target = projectImport(filename, specifier);
    if (!target) continue;

    if (!isFeature && !isRoute && target.startsWith("web/src/lib/features/")) {
      violations.push(
        `${filename}: shared code must not import feature ${specifier}`,
      );
    }

    if (isFeature && target.startsWith("web/src/routes/")) {
      violations.push(`${filename}: features must not import routes`);
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
    violations.push(...frontendSourceViolations(filename, source));
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
