/** Test-owned middleware, subprocesses and fixture data; never attach to development. */
import { spawn, type ChildProcess } from 'node:child_process';
import { openSync, closeSync } from 'node:fs';
import {
  mkdir,
  readFile,
  writeFile,
  copyFile,
  rm,
  mkdtemp,
  open,
} from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { parseEnv } from 'node:util';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import pg from 'pg';

export const repositoryRoot = fileURLToPath(
  new URL('../../../', import.meta.url),
);
const testDirectory = path.join(repositoryRoot, '.local/tests');
const infrastructureDirectory = path.join(testDirectory, 'infrastructure');
const infrastructureEnv = path.join(infrastructureDirectory, 'test.env');
type Environment = Record<string, string>;

async function readEnvironment(file: string): Promise<Environment> {
  const values: Environment = {};
  for (const [key, value] of Object.entries(
    parseEnv(await readFile(file, 'utf8')),
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
  const log = options.logFile ? openSync(options.logFile, 'a') : undefined;
  let child: ChildProcess;
  try {
    child = spawn(command, args, {
      cwd: options.cwd ?? repositoryRoot,
      env: options.env ?? process.env,
      windowsHide: true,
      // Unix needs a separate signal group. Windows detached mode creates another
      // console even with windowsHide, so Windows commands stay attached and hidden.
      detached: process.platform !== 'win32',
      stdio: [
        'ignore',
        options.capture ? 'pipe' : (log ?? 'inherit'),
        log ?? 'inherit',
      ],
    });
  } finally {
    if (log !== undefined) closeSync(log);
  }
  let output = '';
  child.stdout?.on('data', (chunk: Buffer) => {
    output += chunk.toString();
  });

  return new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => {
      if (code === 0) resolve(output);
      else
        reject(
          new Error(
            `${path.basename(command)} failed with exit code ${code ?? signal}${options.logFile ? `; inspect ${options.logFile}` : ''}`,
          ),
        );
    });
  });
}

export function nativeBinaryPath(
  name: string,
  platform: NodeJS.Platform = process.platform,
) {
  return path.join(
    repositoryRoot,
    'target/debug',
    name + (platform === 'win32' ? '.exe' : ''),
  );
}

export async function writeEnvironment(file: string, values: Environment) {
  await writeFile(
    file,
    Object.entries(values)
      .map(([key, value]) => `${key}=${value}`)
      .join('\n') + '\n',
    { mode: 0o600 },
  );
}

function storageConfiguration(values: Environment) {
  return JSON.stringify({
    identities: [
      {
        name: 'test',
        credentials: [
          {
            accessKey: values.SEAWEED_ACCESS_KEY,
            secretKey: values.SEAWEED_SECRET_KEY,
          },
        ],
        actions: ['Admin', 'Read', 'Write', 'List', 'Tagging'],
      },
    ],
  });
}

export async function testEnvironment(): Promise<Environment> {
  await mkdir(infrastructureDirectory, { recursive: true });
  const values = {
    ...(await readEnvironment(path.join(repositoryRoot, '.env.example'))),
    POSTGRES_PASSWORD: 'chat-test-only',
    DATABASE_URL: 'postgres://chat:chat-test-only@127.0.0.1:55433/chat',
    VALKEY_URL: 'redis://127.0.0.1:56380',
    CENTRIFUGO_API_URL: 'http://127.0.0.1:8001/api',
    CENTRIFUGO_API_KEY: 'chat-test-realtime',
    INTERNAL_PROXY_KEY: 'chat-test-proxy',
    SEAWEED_ACCESS_KEY: 'chat-test-media',
    SEAWEED_SECRET_KEY: 'chat-test-media-secret',
    ALLOWED_ORIGINS: 'http://localhost:5174',
    BIND_ADDR: '127.0.0.1:18080',
    // Linux containers reach host-gateway rather than the host loopback listener.
    INTERNAL_BIND_ADDR: `${process.platform === 'linux' ? '0.0.0.0' : '127.0.0.1'}:18081`,
    WORKER_METRICS_ADDR: '127.0.0.1:18082',
    MEDIA_S3_INTERNAL_ENDPOINT: 'http://127.0.0.1:8334',
    MEDIA_S3_PUBLIC_ENDPOINT: 'http://localhost:8337',
    CHAT_MEDIA_ORIGIN: 'http://localhost:8337',
    POSTGRES_PORT: '55433',
    VALKEY_PORT: '56380',
    CENTRIFUGO_PORT: '8001',
    INTERNAL_PROXY_PORT: '18081',
    SEAWEED_S3_PORT: '8334',
    MEDIA_GATEWAY_PORT: '8337',
    SEAWEED_S3_CONFIG: path
      .join(infrastructureDirectory, 's3.json')
      .replaceAll('\\', '/'),
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
    'docker',
    ['compose', '--project-name', project, '--env-file', envFile, ...args],
    {
      env: { ...process.env, ...values },
      capture,
      logFile: path.join(path.dirname(envFile), 'compose.log'),
    },
  );
}

/** The check process owns middleware; child test suites borrow it through an env-file reference. */
export async function prepareTestInfrastructure() {
  await mkdir(testDirectory, { recursive: true });
  const lockFile = path.join(testDirectory, 'infrastructure.lock');
  const lock = await open(lockFile, 'wx').catch(() => {
    throw new Error(
      'Another check owns test infrastructure; run test suites serially. See development.md for interrupted-run cleanup.',
    );
  });
  let values: Environment | undefined;
  const cleanup = async () => {
    try {
      if (values)
        await compose('chat-test', infrastructureEnv, values, ['down']);
    } finally {
      await lock.close();
      await rm(lockFile);
    }
  };
  try {
    values = await testEnvironment();
    await compose('chat-test', infrastructureEnv, values, [
      'up',
      '-d',
      '--wait',
      '--wait-timeout',
      '90',
    ]);
  } catch (error) {
    try {
      await cleanup();
    } catch (cleanupError) {
      throw new AggregateError(
        [error, cleanupError],
        'Infrastructure startup and cleanup failed',
      );
    }
    throw error;
  }
  return { values, envFile: infrastructureEnv, cleanup };
}

type NativeProcess = {
  child: ChildProcess;
  closed: Promise<void>;
  done: boolean;
  failure?: Error;
  code: number | null;
  shutdownRequested: boolean;
  exitDuringShutdown?: boolean;
};

function launch(
  command: string,
  args: string[],
  env: Environment,
  logFile: string,
  cwd = repositoryRoot,
): NativeProcess {
  const log = openSync(logFile, 'a');
  const child = spawn(command, args, {
    cwd,
    env: { ...process.env, ...env },
    windowsHide: true,
    stdio: ['ignore', log, log],
  });
  closeSync(log);
  let closed!: () => void;
  const processState: NativeProcess = {
    child,
    closed: new Promise((resolve) => {
      closed = resolve;
    }),
    done: false,
    code: null,
    shutdownRequested: false,
  };
  child.once('error', (error) => {
    processState.failure = error;
    processState.done = true;
    closed();
  });
  child.once('close', (code) => {
    processState.code = code;
    processState.exitDuringShutdown = processState.shutdownRequested;
    processState.done = true;
    closed();
  });
  return processState;
}

async function waitForExit(processState: NativeProcess, timeout = 90000) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      processState.closed,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () =>
            reject(
              new Error('Owned test process did not stop before the deadline'),
            ),
          timeout,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
  if (processState.failure) throw processState.failure;
}

async function ready(
  url: string,
  processState: NativeProcess,
  headers?: Record<string, string>,
) {
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) {
    if (processState.failure) throw processState.failure;
    if (processState.done)
      throw new Error(
        `Owned test process exited with code ${processState.code}; inspect its log`,
      );
    try {
      if ((await fetch(url, { headers, signal: AbortSignal.timeout(2000) })).ok)
        return;
    } catch {
      /* Connection refusal is expected during startup; the deadline bounds retry. */
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Test listener did not become ready: ${url}`);
}

/** Own one fixture's database, bucket and child objects. No PID files or global process lookup. */
export class TestRuntime {
  readonly directory: string;
  readonly envFile: string;
  readonly env: Environment;
  readonly project: string;
  readonly database: string;
  readonly db: pg.Client;
  readonly apiUrl: string;
  readonly wsUrl: string;
  readonly origin: string;
  readonly marker: string;
  readonly publishedMarker: string;
  private apiProcess?: NativeProcess;
  private workerProcess?: NativeProcess;
  private webProcess?: NativeProcess;
  private infrastructureCleanup?: () => Promise<void>;
  private ownsData: boolean;
  private databaseCreated = false;
  private bucketCreated = false;

  constructor(
    directory: string,
    env: Environment,
    project = 'chat-test',
    ownsData = true,
  ) {
    this.directory = directory;
    this.envFile = path.join(directory, 'stack.env');
    this.marker = path.join(directory, 'maintenance.stop');
    this.publishedMarker = path.join(directory, 'published-marker.json');
    this.env = { ...env, CHAT_SHUTDOWN_FILE: this.marker };
    this.project = project;
    this.database = new URL(env.DATABASE_URL).pathname.slice(1);
    this.db = new pg.Client({ connectionString: env.DATABASE_URL });
    this.apiUrl = `http://${env.BIND_ADDR}`;
    this.wsUrl = env.CENTRIFUGO_API_URL.replace('http:', 'ws:').replace(
      '/api',
      '/connection/websocket',
    );
    this.origin = env.ALLOWED_ORIGINS;
    this.ownsData = ownsData;
  }

  static async start() {
    const borrowed = process.env.CHAT_TEST_INFRASTRUCTURE_ENV;
    const infrastructure = borrowed
      ? undefined
      : await prepareTestInfrastructure();
    let runtime: TestRuntime | undefined;
    try {
      const base = await readEnvironment(borrowed ?? infrastructure!.envFile);
      if (!base.DATABASE_URL || !base.POSTGRES_USER || !base.POSTGRES_DB) {
        throw new Error(
          'Test infrastructure environment is missing database configuration',
        );
      }

      const identity = randomUUID().replaceAll('-', '');
      const directory = await mkdtemp(path.join(testDirectory, 'run-'));
      const databaseUrl = new URL(base.DATABASE_URL);
      databaseUrl.pathname = `/chat_e2e_${identity}`;
      runtime = new TestRuntime(directory, {
        ...base,
        DATABASE_URL: databaseUrl.toString(),
        MEDIA_S3_BUCKET: `chat-e2e-${identity}`,
      });
      runtime.infrastructureCleanup = infrastructure?.cleanup;

      await writeEnvironment(runtime.envFile, runtime.env);
      await runtime.service('up', '-d', '--wait', '--wait-timeout', '90');
      await runtime.service(
        'exec',
        '-T',
        'postgres',
        'psql',
        '-U',
        base.POSTGRES_USER,
        '-d',
        base.POSTGRES_DB,
        '-c',
        `CREATE DATABASE ${runtime.database}`,
      );
      runtime.databaseCreated = true;

      await runCommand('cargo', ['build', '--bins'], {
        logFile: path.join(directory, 'build.log'),
      });
      await runCommand(nativeBinaryPath('admin'), ['media-init'], {
        env: { ...process.env, ...runtime.env },
        logFile: path.join(directory, 'media-init.log'),
      });
      runtime.bucketCreated = true;

      await runtime.db.connect();
      await runtime.startApi();
      return runtime;
    } catch (error) {
      try {
        if (runtime) await runtime.cleanup();
        else await infrastructure?.cleanup();
      } catch (cleanupError) {
        throw new AggregateError(
          [error, cleanupError],
          'Fixture startup and cleanup failed',
        );
      }
      throw error;
    }
  }

  service(...args: string[]) {
    return compose(this.project, this.envFile, this.env, args);
  }

  private async startNative(name: string, env = this.env) {
    // Unique copies allow subsequent Cargo builds to replace binaries on Windows.
    const executable = path.join(
      this.directory,
      `${name}-${randomUUID()}${process.platform === 'win32' ? '.exe' : ''}`,
    );
    await copyFile(nativeBinaryPath(name), executable);
    return launch(
      executable,
      [],
      env,
      path.join(this.directory, `${name}.log`),
    );
  }

  async startApi() {
    if (this.apiProcess && !this.apiProcess.done) {
      throw new Error('Stop the owned API before starting another');
    }
    this.apiProcess = await this.startNative('api');
    await ready(`${this.apiUrl}/health/ready`, this.apiProcess);
  }

  async startWorker(pauseAfterPublish = false) {
    if (this.workerProcess && !this.workerProcess.done)
      throw new Error('Stop the owned worker before starting another');
    await runCommand(
      'cargo',
      [
        'build',
        '--bin',
        'worker',
        ...(pauseAfterPublish ? ['--features', 'test-failpoints'] : []),
      ],
      { logFile: path.join(this.directory, 'worker-build.log') },
    );
    await rm(this.publishedMarker, { force: true });
    const env = { ...this.env };
    if (pauseAfterPublish)
      env.CHAT_TEST_PAUSE_AFTER_PUBLISH_MARKER = this.publishedMarker;
    this.workerProcess = await this.startNative('worker', env);
    await ready(
      `http://${env.WORKER_METRICS_ADDR}/metrics`,
      this.workerProcess,
      { 'X-Proxy-Key': env.INTERNAL_PROXY_KEY },
    );
  }

  async startWeb() {
    this.webProcess = launch(
      process.execPath,
      [
        path.join(repositoryRoot, 'web/node_modules/vite/bin/vite.js'),
        '--host',
        'localhost',
        '--port',
        '5174',
        '--strictPort',
      ],
      {
        ...this.env,
        CHAT_API_PROXY: this.apiUrl,
        CHAT_CENTRIFUGO_PROXY: this.env.CENTRIFUGO_API_URL.replace('/api', ''),
      },
      path.join(this.directory, 'web.log'),
      path.join(repositoryRoot, 'web'),
    );
    await ready(this.origin, this.webProcess);
  }

  /** Deliberate SIGKILL models publish-before-ack crashes on both Windows and Unix. */
  async stopWorker(crash = true) {
    if (!this.workerProcess) return;
    if (crash && !this.workerProcess.done)
      this.workerProcess.child.kill('SIGKILL');
    await waitForExit(this.workerProcess);
    this.workerProcess = undefined;
  }

  get hasWorker() {
    return !!this.workerProcess && !this.workerProcess.done;
  }

  /** Both writers must drain before a recovery snapshot can begin. */
  async stopWriters() {
    const writers = [this.apiProcess, this.workerProcess].filter(
      (value): value is NativeProcess => !!value,
    );
    // Capture shutdown intent before creating the marker so an exit retains its phase.
    for (const writer of writers) writer.shutdownRequested = true;
    await writeFile(this.marker, 'test maintenance');

    await Promise.all(writers.map((writer) => waitForExit(writer)));
    const failed = writers
      .filter((writer) => writer.code !== 0)
      .map((writer) => {
        const phase = writer.exitDuringShutdown
          ? 'during drain'
          : 'before drain';
        return `${path.basename(writer.child.spawnfile)} exit=${writer.code} signal=${writer.child.signalCode} ${phase}`;
      });
    if (failed.length) {
      throw new Error(
        `Test writer did not complete graceful shutdown: ${failed.join(', ')}`,
      );
    }
    this.apiProcess = undefined;
    this.workerProcess = undefined;
    await rm(this.marker);
  }

  async cleanup() {
    const failures: unknown[] = [];
    const attempt = async (operation: () => Promise<unknown>) => {
      try {
        await operation();
      } catch (error) {
        failures.push(error);
      }
    };
    await attempt(() => this.stopWriters());
    // A failed drain is a test failure, but it must not leave native writers alive.
    for (const writer of [this.apiProcess, this.workerProcess]) {
      if (writer && !writer.done) {
        writer.child.kill('SIGKILL');
        await attempt(() => waitForExit(writer, 5000));
      }
    }
    if (this.webProcess && !this.webProcess.done) {
      this.webProcess.child.kill('SIGTERM');
      await attempt(() => waitForExit(this.webProcess!));
    }
    await attempt(() => this.db.end());
    if (this.ownsData) {
      await attempt(() => this.service('stop', 'media-gateway'));
      if (this.bucketCreated)
        await attempt(() =>
          runCommand(nativeBinaryPath('admin'), ['media-cleanup-test'], {
            env: { ...process.env, ...this.env },
            logFile: path.join(this.directory, 'cleanup.log'),
          }),
        );
      if (this.databaseCreated)
        await attempt(() => {
          if (!/^chat_e2e_[a-f0-9]{32}$/.test(this.database)) {
            throw new Error('Unowned database cleanup refused');
          }
          return this.service(
            'exec',
            '-T',
            'postgres',
            'psql',
            '-U',
            this.env.POSTGRES_USER,
            '-d',
            this.env.POSTGRES_DB,
            '-c',
            `DROP DATABASE IF EXISTS ${this.database} WITH (FORCE)`,
          );
        });
    }
    if (this.infrastructureCleanup) await attempt(this.infrastructureCleanup);
    if (failures.length)
      throw new AggregateError(
        failures,
        `Test cleanup failed; inspect ${this.directory}`,
      );
  }
}
