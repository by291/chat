import type { components } from '#lib/api-client';

import { securityToken } from '../verification/email-verification.svelte.ts';

type ResetPassword = (
  input: components['schemas']['PasswordResetInput'],
  signal?: AbortSignal,
) => Promise<void>;

/** Owns the reset fragment and pending request only for this public route's lifetime. */
export class PasswordResetState {
  token = $state('');
  pending = $state(false);
  done = $state(false);
  error = $state('');

  private generation = 0;
  private request?: AbortController;

  constructor(private submit: ResetPassword) {}

  initialize(hash: string) {
    this.dispose();
    this.done = false;
    this.token = securityToken(hash);
    this.error = this.token ? '' : '链接无效或已失效，请重新申请';
  }

  dispose() {
    this.generation++;
    this.request?.abort();
    this.request = undefined;
    this.token = '';
    this.pending = false;
    this.error = '';
  }

  async reset(password: string, confirmation: string): Promise<void> {
    if (!this.token || this.pending || this.done) return;

    if (password !== confirmation) {
      this.error = '两次输入的新密码不一致';
      return;
    }

    const passwordLength = Array.from(password).length;
    if (passwordLength < 6 || passwordLength > 128) {
      this.error = '密码需要 6–128 个字符';
      return;
    }

    const generation = ++this.generation;
    const request = new AbortController();
    this.request = request;
    this.pending = true;
    this.error = '';

    try {
      await this.submit(
        { token: this.token, new_password: password },
        request.signal,
      );
      if (generation === this.generation) {
        this.done = true;
        this.token = '';
      }
    } catch (error) {
      if (generation === this.generation) {
        this.error =
          error instanceof Error ? error.message : '重置失败，请重试';
      }
    } finally {
      if (generation === this.generation) {
        this.pending = false;
        this.request = undefined;
      }
    }
  }
}
