import { expect, test } from 'vitest';

import {
  EmailVerificationState,
  securityToken,
} from '../../src/lib/features/auth/verification/email-verification.svelte';
import { ApiFailure } from '../../src/lib/api-client';

const token = 'a'.repeat(64);
const status = {
  enabled: true,
  verified_email: 'alice@example.test',
  pending_email: null,
  verification_expires_at: null,
};

test('verification accepts only the opaque fragment token and never a query redirect', () => {
  expect(securityToken(`#token=${token}`)).toBe(token);
  expect(securityToken(`?next=%2Fverify-email%23token%3D${token}`)).toBe('');
  expect(securityToken('#token=invalid')).toBe('');
});

test('successful verification consumes only in-memory state and clears the token', async () => {
  let submitted = '';
  const state = new EmailVerificationState(
    async (value) => {
      submitted = value;
      return status;
    },
    () => 'alice',
  );
  state.initialize(`#token=${token}`);

  await state.confirm();

  expect(submitted).toBe(token);
  expect(state.done).toBe(true);
  expect(state.token).toBe('');
  expect(state.pending).toBe(false);
});

test('wrong-account rejection keeps the link available for its initiating account', async () => {
  const state = new EmailVerificationState(
    async () => {
      throw new ApiFailure(403, 'forbidden', '请使用发起绑定的账号');
    },
    () => 'bob',
  );
  state.initialize(`#token=${token}`);

  await state.confirm();

  expect(state.done).toBe(false);
  expect(state.token).toBe(token);
  expect(state.error).toContain('发起绑定');
});

test('account change or teardown fences an old verification acknowledgement', async () => {
  let userId = 'alice';
  let finish!: (value: typeof status) => void;
  const state = new EmailVerificationState(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
    () => userId,
  );
  state.initialize(`#token=${token}`);
  const request = state.confirm();

  userId = 'bob';
  state.invalidate();
  finish(status);
  await request;

  expect(state.done).toBe(false);
  expect(state.pending).toBe(false);
  state.dispose();
  expect(state.token).toBe('');
});
