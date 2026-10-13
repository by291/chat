/** Coordinated recovery drill for fixture-owned data, not a production backup CLI. */
import { randomUUID, createHash, randomBytes } from 'node:crypto';
import {
  readFile,
  writeFile,
  copyFile,
  mkdir,
  mkdtemp,
} from 'node:fs/promises';
import path from 'node:path';

import {
  TestRuntime,
  compose,
  runCommand,
  writeEnvironment,
} from './runtime.ts';

/** Close PUT ingress and drain writers before the database and storage recovery point. */
export async function restoreSnapshot(
  source: TestRuntime,
): Promise<TestRuntime> {
  const backup = path.join(source.directory, 'backup');
  await mkdir(backup, { mode: 0o700 });
  const workerWasRunning = source.hasWorker;
  const seaweed = (
    await compose(
      source.project,
      source.envFile,
      source.env,
      ['ps', '-q', 'seaweedfs'],
      true,
    )
  ).trim();
  if (!seaweed) throw new Error('Source storage is not running');
  let storageStopped = false;
  let writersStopped = false;
  let snapshotError: unknown;

  try {
    await source.service('stop', '--timeout', '90', 'media-gateway');
    await source.stopWriters();
    writersStopped = true;
    await source.service(
      'exec',
      '-T',
      'postgres',
      'pg_dump',
      '-U',
      source.env.POSTGRES_USER,
      '-d',
      source.database,
      '-Fc',
      '-f',
      '/tmp/recovery.dump',
    );
    await source.service(
      'cp',
      'postgres:/tmp/recovery.dump',
      path.join(backup, 'postgres.dump'),
    );
    await source.service('exec', '-T', 'postgres', 'rm', '/tmp/recovery.dump');

    await source.service('stop', 'seaweedfs');
    storageStopped = true;
    await runCommand(
      'docker',
      [
        'run',
        '--rm',
        '--volumes-from',
        `${seaweed}:ro`,
        '--mount',
        `type=bind,source=${backup},target=/backup`,
        'debian:bookworm-slim',
        'tar',
        '-cf',
        '/backup/seaweed.tar',
        '-C',
        '/data',
        '.',
      ],
      { logFile: path.join(source.directory, 'recovery.log') },
    );
    await copyFile(source.envFile, path.join(backup, 'configuration.env'));
    await copyFile(
      source.env.SEAWEED_S3_CONFIG,
      path.join(backup, 'seaweed-s3.json'),
    );
  } catch (error) {
    snapshotError = error;
    throw error;
  } finally {
    // Keep ingress closed until storage and every previously running writer recover.
    // Failed drains leave ownership with source.cleanup(); do not launch duplicate writers.
    try {
      if (storageStopped)
        await source.service(
          'up',
          '-d',
          '--no-recreate',
          '--wait',
          '--wait-timeout',
          '60',
          'seaweedfs',
        );
      if (writersStopped) {
        await source.startApi();
        if (workerWasRunning) await source.startWorker();
        await source.service('start', 'media-gateway');
      }
    } catch (resumeError) {
      if (snapshotError) {
        throw new AggregateError(
          [snapshotError, resumeError],
          'Snapshot and writer restart failed',
        );
      }
      throw resumeError;
    }
  }

  const files = [
    'postgres.dump',
    'seaweed.tar',
    'configuration.env',
    'seaweed-s3.json',
  ];
  const hashes: Record<string, string> = {};
  for (const file of files)
    hashes[file] = createHash('sha256')
      .update(await readFile(path.join(backup, file)))
      .digest('hex');
  await writeFile(
    path.join(backup, 'manifest.json'),
    JSON.stringify({
      schema: 1,
      database: source.database,
      hashes,
      coordinatedWriters: true,
    }),
  );
  for (const file of files) {
    if (
      createHash('sha256')
        .update(await readFile(path.join(backup, file)))
        .digest('hex') !== hashes[file]
    )
      throw new Error(`Recovery hash mismatch: ${file}`);
  }

  const project = `chat-restore-drill-${randomUUID().replaceAll('-', '')}`;
  const containers = await runCommand(
    'docker',
    ['ps', '-aq', '--filter', `label=com.docker.compose.project=${project}`],
    { capture: true },
  );
  const volumes = await runCommand(
    'docker',
    [
      'volume',
      'ls',
      '-q',
      '--filter',
      `label=com.docker.compose.project=${project}`,
    ],
    { capture: true },
  );
  if (containers.trim() || volumes.trim())
    throw new Error('Recovery target already exists');
  const directory = await mkdtemp(
    path.join(path.dirname(source.directory), 'restore-'),
  );
  const password = randomBytes(32).toString('hex');
  const env: Record<string, string> = {
    ...source.env,
    POSTGRES_DB: source.database,
    POSTGRES_PASSWORD: password,
    POSTGRES_PORT: '55435',
    DATABASE_URL: `postgres://${source.env.POSTGRES_USER}:${password}@127.0.0.1:55435/${source.database}`,
    BIND_ADDR: '127.0.0.1:19090',
    INTERNAL_BIND_ADDR: `${process.platform === 'linux' ? '0.0.0.0' : '127.0.0.1'}:19091`,
    WORKER_METRICS_ADDR: '127.0.0.1:19092',
    INTERNAL_PROXY_PORT: '19091',
    VALKEY_PORT: '56381',
    VALKEY_URL: 'redis://127.0.0.1:56381',
    CENTRIFUGO_PORT: '8002',
    CENTRIFUGO_API_URL: 'http://127.0.0.1:8002/api',
    SEAWEED_S3_PORT: '8338',
    SEAWEED_S3_CONFIG: path
      .join(backup, 'seaweed-s3.json')
      .replaceAll('\\', '/'),
    MEDIA_S3_INTERNAL_ENDPOINT: 'http://127.0.0.1:8338',
    MEDIA_S3_PUBLIC_ENDPOINT: 'http://127.0.0.1:8338',
    MEDIA_GATEWAY_PORT: '8339',
  };
  const restored = new TestRuntime(directory, env, project, false);
  try {
    await writeEnvironment(restored.envFile, restored.env);
    await restored.service('up', '-d', '--wait', 'postgres');
    await restored.service(
      'cp',
      path.join(backup, 'postgres.dump'),
      'postgres:/tmp/recovery.dump',
    );
    await restored.service(
      'exec',
      '-T',
      'postgres',
      'pg_restore',
      '-U',
      env.POSTGRES_USER,
      '-d',
      source.database,
      '--exit-on-error',
      '/tmp/recovery.dump',
    );
    await restored.service('create', 'seaweedfs');
    const targetStorage = (
      await compose(
        project,
        restored.envFile,
        env,
        ['ps', '-aq', 'seaweedfs'],
        true,
      )
    ).trim();
    if (!targetStorage)
      throw new Error('Recovery storage container was not created');
    await runCommand(
      'docker',
      [
        'run',
        '--rm',
        '--volumes-from',
        targetStorage,
        '--mount',
        `type=bind,source=${backup},target=/backup,readonly`,
        'debian:bookworm-slim',
        'tar',
        '-xf',
        '/backup/seaweed.tar',
        '-C',
        '/data',
      ],
      { logFile: path.join(directory, 'recovery.log') },
    );
    await restored.service(
      'up',
      '-d',
      '--wait',
      '--wait-timeout',
      '60',
      'seaweedfs',
      'valkey',
      'centrifugo',
    );
    await restored.db.connect();
    await restored.startApi();
    return restored;
  } catch (error) {
    try {
      await cleanupRestored(restored);
    } catch (cleanupError) {
      throw new AggregateError(
        [error, cleanupError],
        'Recovery startup and cleanup failed',
      );
    }
    throw error;
  }
}

/** Only the unique drill project may have its volumes removed. */
export async function cleanupRestored(restored: TestRuntime) {
  if (!/^chat-restore-drill-[a-f0-9]{32}$/.test(restored.project))
    throw new Error('Unowned recovery project cleanup refused');
  const failures: unknown[] = [];
  try {
    await restored.cleanup();
  } catch (error) {
    failures.push(error);
  }
  try {
    await restored.service('down', '--volumes');
  } catch (error) {
    failures.push(error);
  }
  if (failures.length) {
    throw new AggregateError(failures, 'Recovery cleanup failed');
  }
}
