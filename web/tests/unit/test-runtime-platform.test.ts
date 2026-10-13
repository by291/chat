import { spawn } from 'node:child_process';

import { expect, test, vi } from 'vitest';

import { runCommand } from '../integration/runtime';

vi.mock('node:child_process', async () => {
  const { EventEmitter } = await import('node:events');
  return {
    spawn: vi.fn(() => {
      const child = new EventEmitter();
      queueMicrotask(() => child.emit('close', 0, null));
      return child;
    }),
  };
});

test.each([
  { platform: 'win32', detached: false },
  { platform: 'linux', detached: true },
  { platform: 'darwin', detached: true },
] as const)(
  '$platform commands use the correct console and signal policy',
  async ({ platform, detached }) => {
    const original = Object.getOwnPropertyDescriptor(process, 'platform')!;
    Object.defineProperty(process, 'platform', {
      ...original,
      value: platform,
    });
    try {
      await runCommand('fixture-command', [], { capture: true });

      // A hidden Windows command must stay attached: detached creates a new console.
      // Unix commands need their own group so Ctrl+C can drain the owning check first.
      expect(vi.mocked(spawn).mock.calls.at(-1)?.[2]).toMatchObject({
        windowsHide: true,
        detached,
      });
    } finally {
      Object.defineProperty(process, 'platform', original);
    }
  },
);
