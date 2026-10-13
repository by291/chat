import path from 'node:path';

import { expect, test } from 'vitest';

import * as runtime from '../integration/runtime';

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

test('missing test executables report spawn failure', async () => {
  const operation = runtime.runCommand('chat-missing-test-executable', [], {
    capture: true,
  });

  await expect(operation).rejects.toThrow();
});

test('Rust executable paths work for Windows and Unix hosts', () => {
  const windows = runtime.nativeBinaryPath('api', 'win32');
  const linux = runtime.nativeBinaryPath('api', 'linux');

  expect(path.basename(windows)).toBe('api.exe');
  expect(path.basename(linux)).toBe('api');
});
