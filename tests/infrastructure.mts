/** Shared test middleware and argv execution; uses only Node built-ins.
 * The .mts extension keeps ESM explicit for Node and Playwright outside web's package scope.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { openSync, closeSync, fstatSync } from "node:fs";
import { mkdir, readFile, writeFile, rm, open } from "node:fs/promises";
import { parseEnv } from "node:util";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const repositoryRoot = fileURLToPath(new URL("../", import.meta.url));
export const testDirectory = path.join(repositoryRoot, ".local/tests");
const infrastructureDirectory = path.join(testDirectory, "infrastructure");
const infrastructureEnv = path.join(infrastructureDirectory, "test.env");
const diagnosticTailBytes = 8_192;
const diagnosticTimeoutMs = 15_000;
const terminationTimeoutMs = 1_000;

export type Environment = Record<string, string>;

function redactOutput(output: string, secrets: string[]) {
  for (const secret of [...secrets]
    .filter(Boolean)
    .sort((left, right) => right.length - left.length)) {
    output = output.replaceAll(secret, "[REDACTED]");
  }
  return output;
}

function diagnosticOutput(output: string, secrets: string[]) {
  const buffer = Buffer.from(redactOutput(output, secrets), "utf8");
  let offset = Math.max(0, buffer.length - diagnosticTailBytes);
  // Start at a complete UTF-8 character while keeping the byte limit exact.
  while ((buffer[offset] & 0xc0) === 0x80) offset += 1;
  return buffer.subarray(offset).toString("utf8");
}

async function commandLogTail(file: string, startOffset: number) {
  const log = await open(file, "r");
  try {
    const { size } = await log.stat();
    const offset = Math.max(startOffset, size - diagnosticTailBytes);
    const buffer = Buffer.alloc(size - offset);
    await log.read(buffer, 0, buffer.length, offset);

    const output = buffer.toString("utf8");
    // Discard the first partial line: truncation must not reveal part of a secret.
    if (offset > startOffset) {
      const firstNewline = output.indexOf("\n");
      return firstNewline < 0 ? "" : output.slice(firstNewline + 1);
    }
    return output;
  } finally {
    await log.close();
  }
}

function terminateOwnedCommand(child: ChildProcess) {
  if (child.pid === undefined) return;

  if (process.platform !== "win32") {
    try {
      // The group can still contain descendants after its leader has exited.
      process.kill(-child.pid, "SIGKILL");
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ESRCH") {
        child.kill("SIGKILL");
      }
    }
    return;
  }

  const killRunningChild = () => {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGKILL");
    }
  };
  // Do not hand an already-exited PID to taskkill: Windows may reuse that ID.
  if (child.exitCode !== null || child.signalCode !== null) return;

  const taskkill = path.join(
    process.env.SystemRoot ?? "C:\\Windows",
    "System32",
    "taskkill.exe",
  );
  const terminator = spawn(taskkill, ["/PID", String(child.pid), "/T", "/F"], {
    windowsHide: true,
    detached: false,
    stdio: "ignore",
  });
  const deadline = setTimeout(() => {
    terminator.kill("SIGKILL");
    killRunningChild();
  }, terminationTimeoutMs);
  deadline.unref();
  terminator.unref();
  terminator.once("error", () => {
    // Missing/failed taskkill still permits terminating the owned process handle.
    clearTimeout(deadline);
    killRunningChild();
  });
  terminator.once("close", (code) => {
    clearTimeout(deadline);
    if (code !== 0) killRunningChild();
  });
}

export async function readEnvironment(file: string): Promise<Environment> {
  const values: Environment = {};
  for (const [key, value] of Object.entries(
    parseEnv(await readFile(file, "utf8")),
  )) {
    if (value !== undefined) values[key] = value;
  }
  return values;
}

/** Execute argv directly and reject timed commands independently of inherited stdio closure. */
export async function runCommand(
  command: string,
  args: string[],
  options: {
    cwd?: string;
    env?: NodeJS.ProcessEnv;
    capture?: boolean;
    logFile?: string;
    redact?: string[];
    timeoutMs?: number;
  } = {},
): Promise<string> {
  const log = options.logFile
    ? openSync(options.logFile, "a", 0o600)
    : undefined;
  const startOffset = log === undefined ? 0 : fstatSync(log).size;
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
    let settled = false;
    const fail = (error: unknown) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      reject(error);
    };

    const timeout = options.timeoutMs
      ? setTimeout(() => {
          const error = new Error(
            `${path.basename(command)} timed out after ${options.timeoutMs}ms${options.logFile ? `; inspect ${options.logFile}` : ""}`,
          );
          // Settlement must not wait for inherited pipes or a delayed log read.
          // The private raw log remains available when the deadline cuts it short.
          fail(error);
          try {
            terminateOwnedCommand(child);
          } catch (terminationError) {
            error.cause = terminationError;
          } finally {
            child.stdout?.destroy();
            child.stderr?.destroy();
            child.unref();
          }
        }, options.timeoutMs)
      : undefined;
    child.once("error", fail);
    child.once("close", async (code, signal) => {
      if (settled) return;
      if (code === 0) {
        settled = true;
        clearTimeout(timeout);
        resolve(output);
      } else {
        const summary = `${path.basename(command)} failed with exit code ${code ?? signal}`;
        let tail = "";
        if (options.logFile) {
          try {
            tail = diagnosticOutput(
              await commandLogTail(options.logFile, startOffset),
              options.redact ?? [],
            );
          } catch (logError) {
            fail(
              new AggregateError(
                [new Error(summary), logError],
                "Command failed and its log could not be read",
              ),
            );
            return;
          }
        }
        fail(
          new Error(
            `${summary}${options.logFile ? `; inspect ${options.logFile}` : ""}${tail ? `\n${tail}` : ""}`,
          ),
        );
      }
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

/** Run owned Compose commands; retain redacted startup diagnostics before cleanup. */
export async function compose(
  project: string,
  envFile: string,
  values: Environment,
  args: string[],
  capture = false,
) {
  const directory = path.dirname(envFile);
  const secrets = Object.entries(values)
    .filter(([key]) => /PASSWORD|TOKEN|SECRET|KEY|DATABASE_URL/.test(key))
    .map(([, value]) => value);
  const commandArgs = [
    "compose",
    "--project-name",
    project,
    "--env-file",
    envFile,
  ];
  const options = {
    env: { ...process.env, ...values },
    logFile: path.join(directory, "compose.log"),
    redact: secrets,
  };

  // Explicit child environment wins over inherited developer variables in Compose.
  try {
    return await runCommand("docker", [...commandArgs, ...args], {
      ...options,
      capture,
    });
  } catch (error) {
    if (args[0] !== "up") throw error;

    // Collect before cleanup removes the failed containers. Both commands remain
    // scoped to the caller's project; config/env files never enter this artifact.
    const sections = [
      `Compose startup failed for ${project}\n${diagnosticOutput(String(error), secrets)}`,
    ];
    const failures: unknown[] = [];
    for (const diagnosticArgs of [
      ["ps", "--all"],
      ["logs", "--no-color", "--tail", "100"],
    ]) {
      try {
        const output = await runCommand(
          "docker",
          [...commandArgs, ...diagnosticArgs],
          {
            ...options,
            capture: true,
            timeoutMs: diagnosticTimeoutMs,
          },
        );
        sections.push(
          `${diagnosticArgs[0]}\n${diagnosticOutput(output, secrets)}`,
        );
      } catch (diagnosticError) {
        failures.push(diagnosticError);
        sections.push(
          `${diagnosticArgs[0]}\n${diagnosticOutput(String(diagnosticError), secrets)}`,
        );
      }
    }
    const diagnostic = sections.join("\n\n") + "\n";
    console.error(diagnostic);
    try {
      await writeFile(
        path.join(directory, "compose-diagnostics.log"),
        diagnostic,
        { mode: 0o600 },
      );
    } catch (writeError) {
      failures.push(writeError);
    }
    if (failures.length) {
      throw new AggregateError(
        [
          error,
          new AggregateError(failures, "Compose diagnostics collection failed"),
        ],
        "Compose startup and diagnostics failed",
      );
    }
    throw error;
  }
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
