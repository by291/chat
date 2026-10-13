/** Shared test middleware and argv execution; uses only Node built-ins.
 * The .mts extension keeps ESM explicit for Node and Playwright outside web's package scope.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { openSync, closeSync } from "node:fs";
import { mkdir, readFile, writeFile, rm, open } from "node:fs/promises";
import { parseEnv } from "node:util";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const repositoryRoot = fileURLToPath(new URL("../", import.meta.url));
export const testDirectory = path.join(repositoryRoot, ".local/tests");
const infrastructureDirectory = path.join(testDirectory, "infrastructure");
const infrastructureEnv = path.join(infrastructureDirectory, "test.env");
export type Environment = Record<string, string>;

export async function readEnvironment(file: string): Promise<Environment> {
  const values: Environment = {};
  for (const [key, value] of Object.entries(
    parseEnv(await readFile(file, "utf8")),
  )) {
    if (value !== undefined) values[key] = value;
  }
  return values;
}

/** Execute argv directly: no shell quoting, PowerShell dependency or command expansion. */
export async function runCommand(
  command: string,
  args: string[],
  options: {
    cwd?: string;
    env?: NodeJS.ProcessEnv;
    capture?: boolean;
    logFile?: string;
  } = {},
): Promise<string> {
  const log = options.logFile ? openSync(options.logFile, "a") : undefined;
  let child: ChildProcess;
  try {
    child = spawn(command, args, {
      cwd: options.cwd ?? repositoryRoot,
      env: options.env ?? process.env,
      windowsHide: true,
      // Unix needs a separate signal group. Windows detached mode creates another
      // console even with windowsHide, so Windows commands stay attached and hidden.
      detached: process.platform !== "win32",
      stdio: [
        "ignore",
        options.capture ? "pipe" : (log ?? "inherit"),
        log ?? "inherit",
      ],
    });
  } finally {
    if (log !== undefined) closeSync(log);
  }
  let output = "";
  child.stdout?.on("data", (chunk: Buffer) => {
    output += chunk.toString();
  });

  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (code, signal) => {
      if (code === 0) resolve(output);
      else
        reject(
          new Error(
            `${path.basename(command)} failed with exit code ${code ?? signal}${options.logFile ? `; inspect ${options.logFile}` : ""}`,
          ),
        );
    });
  });
}

export async function writeEnvironment(file: string, values: Environment) {
  await writeFile(
    file,
    Object.entries(values)
      .map(([key, value]) => `${key}=${value}`)
      .join("\n") + "\n",
    { mode: 0o600 },
  );
}

function storageConfiguration(values: Environment) {
  return JSON.stringify({
    identities: [
      {
        name: "test",
        credentials: [
          {
            accessKey: values.SEAWEED_ACCESS_KEY,
            secretKey: values.SEAWEED_SECRET_KEY,
          },
        ],
        actions: ["Admin", "Read", "Write", "List", "Tagging"],
      },
    ],
  });
}

export async function testEnvironment(): Promise<Environment> {
  await mkdir(infrastructureDirectory, { recursive: true });
  const values = {
    ...(await readEnvironment(path.join(repositoryRoot, ".env.example"))),
    POSTGRES_PASSWORD: "chat-test-only",
    DATABASE_URL: "postgres://chat:chat-test-only@127.0.0.1:55433/chat",
    VALKEY_URL: "redis://127.0.0.1:56380",
    CENTRIFUGO_API_URL: "http://127.0.0.1:8001/api",
    CENTRIFUGO_API_KEY: "chat-test-realtime",
    INTERNAL_PROXY_KEY: "chat-test-proxy",
    SEAWEED_ACCESS_KEY: "chat-test-media",
    SEAWEED_SECRET_KEY: "chat-test-media-secret",
    ALLOWED_ORIGINS: "http://localhost:5174",
    BIND_ADDR: "127.0.0.1:18080",
    // Linux containers reach host-gateway rather than the host loopback listener.
    INTERNAL_BIND_ADDR: `${process.platform === "linux" ? "0.0.0.0" : "127.0.0.1"}:18081`,
    WORKER_METRICS_ADDR: "127.0.0.1:18082",
    MEDIA_S3_INTERNAL_ENDPOINT: "http://127.0.0.1:8334",
    MEDIA_S3_PUBLIC_ENDPOINT: "http://localhost:8337",
    CHAT_MEDIA_ORIGIN: "http://localhost:8337",
    POSTGRES_PORT: "55433",
    VALKEY_PORT: "56380",
    CENTRIFUGO_PORT: "8001",
    INTERNAL_PROXY_PORT: "18081",
    SEAWEED_S3_PORT: "8334",
    MEDIA_GATEWAY_PORT: "8337",
    SEAWEED_S3_CONFIG: path
      .join(infrastructureDirectory, "s3.json")
      .replaceAll("\\", "/"),
  };
  await writeFile(values.SEAWEED_S3_CONFIG, storageConfiguration(values), {
    mode: 0o600,
  });
  await writeEnvironment(infrastructureEnv, values);
  return values;
}

export async function compose(
  project: string,
  envFile: string,
  values: Environment,
  args: string[],
  capture = false,
) {
  // Explicit child environment wins over inherited developer variables in Compose.
  return runCommand(
    "docker",
    ["compose", "--project-name", project, "--env-file", envFile, ...args],
    {
      env: { ...process.env, ...values },
      capture,
      logFile: path.join(path.dirname(envFile), "compose.log"),
    },
  );
}

/** The check process owns middleware; child test suites borrow it through an env-file reference. */
export async function prepareTestInfrastructure() {
  await mkdir(testDirectory, { recursive: true });
  const lockFile = path.join(testDirectory, "infrastructure.lock");
  const lock = await open(lockFile, "wx").catch(() => {
    throw new Error(
      "Another check owns test infrastructure; run test suites serially. See development.md for interrupted-run cleanup.",
    );
  });
  let values: Environment | undefined;
  const cleanup = async () => {
    try {
      if (values)
        await compose("chat-test", infrastructureEnv, values, ["down"]);
    } finally {
      await lock.close();
      await rm(lockFile);
    }
  };
  try {
    values = await testEnvironment();
    await compose("chat-test", infrastructureEnv, values, [
      "up",
      "-d",
      "--wait",
      "--wait-timeout",
      "90",
    ]);
  } catch (error) {
    try {
      await cleanup();
    } catch (cleanupError) {
      throw new AggregateError(
        [error, cleanupError],
        "Infrastructure startup and cleanup failed",
      );
    }
    throw error;
  }
  return { values, envFile: infrastructureEnv, cleanup };
}
