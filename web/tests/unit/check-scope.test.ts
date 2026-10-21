import { execFile } from 'node:child_process';
import {
  copyFile,
  mkdir,
  mkdtemp,
  rm,
  writeFile,
  access,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';

import { expect, test } from 'vitest';

import { repositoryRoot } from '../../../tests/infrastructure.mts';

const execute = promisify(execFile);

test('frontend checks run without Rust, Docker, pg or infrastructure initialization', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'chat-check-scope-'));
  try {
    await mkdir(path.join(directory, 'scripts'));
    await mkdir(path.join(directory, 'tests'));
    await copyFile(
      path.join(repositoryRoot, 'scripts/check.mjs'),
      path.join(directory, 'scripts/check.mjs'),
    );
    await copyFile(
      path.join(repositoryRoot, 'tests/infrastructure.mts'),
      path.join(directory, 'tests/infrastructure.mts'),
    );
    await writeFile(path.join(directory, 'tests/architecture.test.mjs'), '');
    for (const name of [
      '@sveltejs/kit',
      'svelte-check',
      'typescript',
      'eslint',
      'prettier',
      'vitest',
      'vite',
    ]) {
      const tool = path.join(directory, 'web/node_modules', name);
      await mkdir(tool, { recursive: true });
      await writeFile(
        path.join(tool, 'package.json'),
        JSON.stringify({ bin: 'cli.mjs' }),
      );
      await writeFile(path.join(tool, 'cli.mjs'), 'process.exit(0);');
    }

    // Only absolute Node commands are available; invoking Cargo or Docker must fail.
    const { stdout } = await execute(
      process.execPath,
      [path.join(directory, 'scripts/check.mjs'), '--scope', 'frontend'],
      {
        env: { ...process.env, PATH: '' },
        windowsHide: true,
      },
    );

    expect(stdout).toContain('Checking Svelte types');
    expect(stdout).toContain('Checking Service worker types');
    expect(stdout).toContain('Checking Svelte and TypeScript lint');
    expect(stdout).toContain('Checking Static build');
    expect(stdout).not.toContain('Checking Rust');
    await expect(access(path.join(directory, '.local'))).rejects.toThrow();
  } finally {
    const temporaryRoot = path.resolve(tmpdir()) + path.sep;
    if (
      !path.resolve(directory).startsWith(temporaryRoot) ||
      !path.basename(directory).startsWith('chat-check-scope-')
    ) {
      throw new Error('Unowned test directory cleanup refused');
    }
    await rm(directory, { recursive: true, force: true });
  }
});

test('Playwright discovers browser tests across the shared infrastructure module boundary', async () => {
  const { stdout } = await execute(
    process.execPath,
    [
      path.join(repositoryRoot, 'web/node_modules/@playwright/test/cli.js'),
      'test',
      '--list',
    ],
    { cwd: path.join(repositoryRoot, 'web'), windowsHide: true },
  );

  // Discovery loads the real fixture graph without creating middleware or native processes.
  expect(stdout).toMatch(/Total: [1-9]\d* tests in [1-9]\d* files/);
});
