import { expect, test } from 'vitest';

import { PasswordResetState } from '../../src/lib/features/auth/password-recovery.svelte';

const token = 'b'.repeat(64);

test('reset requires a matching Unicode password without consuming the link on validation failure', async () => {
  let calls = 0;
  const state = new PasswordResetState(async () => {
    calls++;
  });
  state.initialize(`#token=${token}`);

  await state.reset('short', 'short');
  await state.reset('valid password', 'different password');

  expect(calls).toBe(0);
  expect(state.token).toBe(token);
  expect(state.done).toBe(false);
  await state.reset('😀'.repeat(6), '😀'.repeat(6));
  expect(calls).toBe(1);
  expect(state.done).toBe(true);
});

test('successful reset clears the in-memory token and submits no login request', async () => {
  let submitted: unknown;
  const state = new PasswordResetState(async (input) => {
    submitted = input;
  });
  state.initialize(`#token=${token}`);

  await state.reset('new correct password', 'new correct password');

  expect(submitted).toEqual({ token, new_password: 'new correct password' });
  expect(state.token).toBe('');
  expect(state.done).toBe(true);
  expect(state.pending).toBe(false);
});

test('a lost reset acknowledgement leaves login as the recovery path and preserves the token locally', async () => {
  const state = new PasswordResetState(async () => {
    throw new Error('Connection lost');
  });
  state.initialize(`#token=${token}`);

  await state.reset('new correct password', 'new correct password');

  expect(state.done).toBe(false);
  expect(state.token).toBe(token);
  expect(state.error).toContain('Connection lost');
});

test('teardown aborts and fences a late reset response and erases the transient token', async () => {
  let finish!: () => void;
  let signal: AbortSignal | undefined;
  const state = new PasswordResetState((_, value) => {
    signal = value;
    return new Promise((resolve) => {
      finish = resolve;
    });
  });
  state.initialize(`#token=${token}`);
  const request = state.reset('new correct password', 'new correct password');

  state.dispose();
  finish();
  await request;

  expect(signal?.aborted).toBe(true);
  expect(state.token).toBe('');
  expect(state.done).toBe(false);
  expect(state.pending).toBe(false);
});
