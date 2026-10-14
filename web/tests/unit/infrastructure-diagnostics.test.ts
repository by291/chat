import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { expect, test, vi } from 'vitest';

import { compose, writeEnvironment } from '../../../tests/infrastructure.mts';

const fixture = vi.hoisted(() => ({ dockerScript: '' }));

// Exercise the real process/log handling; only the external Docker daemon is replaced.
vi.mock('node:child_process', async (importOriginal) => {
  const original = await importOriginal<typeof import('node:child_process')>();
  return {
    ...original,
    spawn: (command: string, args: string[], options: object) =>
      original.spawn(
        command === 'docker' ? process.execPath : command,
        command === 'docker' ? ['-e', fixture.dockerScript, ...args] : args,
        options,
      ),
  };
});

test('failed Compose startup retains owned service diagnostics without credential files', async () => {
  const directory = await mkdtemp(
    path.join(tmpdir(), 'chat-compose-diagnostics-'),
  );
  const envFile = path.join(directory, 'test.env');
  const secret = 'fixture-storage-secret';
  const values = { SEAWEED_SECRET_KEY: secret };
  fixture.dockerScript = `
    const args = process.argv.slice(1);
    const project = args[args.indexOf('--project-name') + 1];
    const action = args[args.indexOf('--env-file') + 2];
    if (project !== 'chat-ci-unit') process.exit(99);
    if (action === 'up') {
      process.stderr.write('storage startup failed: ' + process.env.SEAWEED_SECRET_KEY + '\\n');
      process.exit(7);
    }
    if (action === 'ps') process.stdout.write('chat-ci-unit-seaweedfs-1 Exited (255)\\n');
    if (action === 'logs') process.stdout.write('界'.repeat(10000) + '\\nseaweedfs | permission denied: ' + process.env.SEAWEED_SECRET_KEY + '\\n');
  `;
  try {
    await writeEnvironment(envFile, values);

    const operation = compose('chat-ci-unit', envFile, values, [
      'up',
      '-d',
      '--wait',
    ]);

    await expect(operation).rejects.toThrow(
      'storage startup failed: [REDACTED]',
    );
    const diagnostic = await readFile(
      path.join(directory, 'compose-diagnostics.log'),
      'utf8',
    );
    expect(diagnostic).toContain('chat-ci-unit-seaweedfs-1 Exited (255)');
    expect(diagnostic).toContain('seaweedfs | permission denied: [REDACTED]');
    expect(diagnostic).not.toContain(secret);
    expect(diagnostic).not.toContain('SEAWEED_SECRET_KEY=');
    expect(Buffer.byteLength(diagnostic, 'utf8')).toBeLessThan(10000);
  } finally {
    const temporaryRoot = path.resolve(tmpdir()) + path.sep;
    if (
      !path.resolve(directory).startsWith(temporaryRoot) ||
      !path.basename(directory).startsWith('chat-compose-diagnostics-')
    ) {
      throw new Error('Unowned test directory cleanup refused');
    }
    await rm(directory, { recursive: true, force: true });
  }
});

test('diagnostic collection failures preserve the original Compose startup error', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'chat-compose-errors-'));
  const envFile = path.join(directory, 'test.env');
  fixture.dockerScript = `
    const args = process.argv.slice(1);
    const action = args[args.indexOf('--env-file') + 2];
    process.stderr.write(action === 'up' ? 'original startup error\\n' : action + ' diagnostics unavailable\\n');
    process.exit(action === 'up' ? 7 : 9);
  `;
  try {
    await writeEnvironment(envFile, {});

    const failure = await compose('chat-ci-unit', envFile, {}, [
      'up',
      '-d',
      '--wait',
    ]).catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(AggregateError);
    const errors = (failure as AggregateError).errors;
    expect(errors[0].message).toContain('original startup error');
    expect(errors[1].message).toContain('diagnostics');
    const diagnostic = await readFile(
      path.join(directory, 'compose-diagnostics.log'),
      'utf8',
    );
    expect(diagnostic).toContain('original startup error');
    expect(diagnostic).toContain('ps diagnostics unavailable');
    expect(diagnostic).toContain('logs diagnostics unavailable');
  } finally {
    const temporaryRoot = path.resolve(tmpdir()) + path.sep;
    if (
      !path.resolve(directory).startsWith(temporaryRoot) ||
      !path.basename(directory).startsWith('chat-compose-errors-')
    ) {
      throw new Error('Unowned test directory cleanup refused');
    }
    await rm(directory, { recursive: true, force: true });
  }
});
