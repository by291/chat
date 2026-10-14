import { ChildProcess } from 'node:child_process';
import path from 'node:path';
import { mkdtemp, readFile, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { PassThrough } from 'node:stream';

import { expect, test, vi } from 'vitest';

import * as runtime from '../integration/runtime';

const fixture = vi.hoisted(() => ({
  heldOutputChild: undefined as ChildProcess | undefined,
  terminations: [] as { args: string[]; options: object }[],
}));

// Node's exit and stdio-close events are independent. Control only that boundary;
// ordinary command tests and native descendant cancellation still spawn real processes.
vi.mock('node:child_process', async (importOriginal) => {
  const original = await importOriginal<typeof import('node:child_process')>();
  return {
    ...original,
    spawn: (command: string, args: string[], options: object) => {
      if (command === 'chat-held-output-fixture')
        return fixture.heldOutputChild;
      if (
        command.toLowerCase().endsWith('taskkill.exe') &&
        fixture.heldOutputChild &&
        args.includes(String(fixture.heldOutputChild.pid))
      ) {
        fixture.terminations.push({ args, options });
        const terminator = new original.ChildProcess();
        queueMicrotask(() => terminator.emit('close', 0, null));
        return terminator;
      }
      return original.spawn(command, args, options);
    },
  };
});

function heldOutputChild(exited: boolean) {
  const child = new ChildProcess();
  Object.defineProperty(child, 'pid', { value: 123_456_789 });
  child.stdout = new PassThrough();
  Object.defineProperty(child, 'exitCode', { value: exited ? 0 : null });
  vi.spyOn(child, 'kill').mockImplementation(() => {
    if (!exited) {
      Object.defineProperty(child, 'signalCode', { value: 'SIGKILL' });
      child.emit('exit', null, 'SIGKILL');
    }
    return !exited;
  });
  return child;
}

test('test commands preserve arguments with spaces and explicit child environment', async () => {
  const value = 'a path with spaces; no shell expansion';
  const result = await runtime.runCommand(
    process.execPath,
    [
      '-e',
      'process.stdout.write(JSON.stringify([process.argv[1], process.env.CHAT_TEST_VALUE]))',
      value,
    ],
    {
      capture: true,
      env: { ...process.env, CHAT_TEST_VALUE: 'fixture value' },
    },
  );

  expect(JSON.parse(result)).toEqual([value, 'fixture value']);
});

test('a failed check rejects instead of continuing with a successful cleanup status', async () => {
  const operation = runtime.runCommand(
    process.execPath,
    ['-e', 'process.exit(7)'],
    { capture: true },
  );

  await expect(operation).rejects.toThrow('exit code 7');
});

test('logged command failures expose a bounded tail and redact credentials', async () => {
  const directory = await mkdtemp(
    path.join(tmpdir(), 'chat-command-diagnostics-'),
  );
  const logFile = path.join(directory, 'command.log');
  const credential = 'fixture-private-key';
  try {
    const operation = runtime.runCommand(
      process.execPath,
      [
        '-e',
        `process.stderr.write('discarded prefix\\n' + 'x'.repeat(20000) + '\\nstorage credentials unreadable: ${credential}\\n'); process.exit(7);`,
      ],
      { logFile, redact: [credential] },
    );

    const failure = await operation.catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(Error);
    expect((failure as Error).message).toContain('exit code 7');
    expect((failure as Error).message).toContain(
      'storage credentials unreadable: [REDACTED]',
    );
    expect((failure as Error).message).not.toContain(credential);
    expect((failure as Error).message).not.toContain('discarded prefix');
    expect((failure as Error).message.length).toBeLessThan(10000);
    expect(await readFile(logFile, 'utf8')).toContain(credential);
    if (process.platform !== 'win32') {
      expect((await stat(logFile)).mode & 0o777).toBe(0o600);
    }
  } finally {
    const temporaryRoot = path.resolve(tmpdir()) + path.sep;
    if (
      !path.resolve(directory).startsWith(temporaryRoot) ||
      !path.basename(directory).startsWith('chat-command-diagnostics-')
    ) {
      throw new Error('Unowned test directory cleanup refused');
    }
    await rm(directory, { recursive: true, force: true });
  }
});

test('truncated command logs discard incomplete credential lines', async () => {
  const directory = await mkdtemp(
    path.join(tmpdir(), 'chat-command-diagnostics-'),
  );
  const credential = 'fixture-secret-'.repeat(800);
  try {
    const operation = runtime.runCommand(
      process.execPath,
      [
        '-e',
        'process.stderr.write(process.env.CHAT_TEST_SECRET); process.exit(7);',
      ],
      {
        logFile: path.join(directory, 'command.log'),
        env: { ...process.env, CHAT_TEST_SECRET: credential },
        redact: [credential],
      },
    );

    const failure = await operation.catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(Error);
    expect((failure as Error).message).toContain('exit code 7');
    expect((failure as Error).message).not.toContain('fixture-secret');
  } finally {
    const temporaryRoot = path.resolve(tmpdir()) + path.sep;
    if (
      !path.resolve(directory).startsWith(temporaryRoot) ||
      !path.basename(directory).startsWith('chat-command-diagnostics-')
    ) {
      throw new Error('Unowned test directory cleanup refused');
    }
    await rm(directory, { recursive: true, force: true });
  }
});

test('missing test executables report spawn failure', async () => {
  const operation = runtime.runCommand('chat-missing-test-executable', [], {
    capture: true,
  });

  await expect(operation).rejects.toThrow();
});

test('diagnostic deadlines also stop descendants holding captured output open', async () => {
  const started = performance.now();
  const operation = runtime.runCommand(
    process.execPath,
    [
      '-e',
      `require('node:child_process').spawn(process.execPath, ['-e', 'setTimeout(() => {}, 2000)'], { stdio: ['ignore', process.stdout, 'ignore'], windowsHide: true }); setInterval(() => {}, 2000);`,
    ],
    { capture: true, timeoutMs: 250 },
  );

  await expect(operation).rejects.toThrow('timed out after 250ms');

  expect(performance.now() - started).toBeLessThan(1500);
});

test.each([false, true])(
  'timeout settles while inherited output remains open (parent exited: %s)',
  async (exited) => {
    const child = heldOutputChild(exited);
    fixture.heldOutputChild = child;
    fixture.terminations = [];
    let watchdog: ReturnType<typeof setTimeout> | undefined;
    const operation = runtime.runCommand('chat-held-output-fixture', [], {
      capture: true,
      timeoutMs: 25,
    });
    try {
      const failure = await Promise.race([
        operation.catch((error: unknown) => error),
        new Promise((resolve) => {
          watchdog = setTimeout(
            () => resolve('waiting for inherited output'),
            500,
          );
        }),
      ]);

      expect(failure).toBeInstanceOf(Error);
      expect((failure as Error).message).toContain('timed out after 25ms');
      expect(child.stdout?.destroyed).toBe(true);
      if (process.platform === 'win32' && !exited) {
        expect(fixture.terminations).toEqual([
          {
            args: ['/PID', String(child.pid), '/T', '/F'],
            options: expect.objectContaining({
              windowsHide: true,
              detached: false,
            }),
          },
        ]);
      } else {
        expect(fixture.terminations).toEqual([]);
      }
    } finally {
      clearTimeout(watchdog);
      child.stdout?.destroy();
      child.emit('close', child.exitCode, child.signalCode);
      await operation.catch(() => undefined);
      fixture.heldOutputChild = undefined;
    }
  },
);

test('Rust executable paths work for Windows and Unix hosts', () => {
  const windows = runtime.nativeBinaryPath('api', 'win32');
  const linux = runtime.nativeBinaryPath('api', 'linux');

  expect(path.basename(windows)).toBe('api.exe');
  expect(path.basename(linux)).toBe('api');
});
