import type { components } from '../../api-client';

type EmailStatus = components['schemas']['EmailStatus'];
type VerifyEmail = (
  token: string,
  signal?: AbortSignal,
) => Promise<EmailStatus>;

/** Only fragments carry security tokens; query redirects and malformed values stay unusable. */
export function securityToken(hash: string): string {
  if (!hash.startsWith('#')) return '';
  const token = new URLSearchParams(hash.slice(1)).get('token') ?? '';
  return /^[a-f\d]{64}$/i.test(token) ? token : '';
}

/** Owns transient token/form state outside the authenticated chat tree. No storage or redirects. */
export class EmailVerificationState {
  token = $state('');
  pending = $state(false);
  done = $state(false);
  error = $state('');
  private generation = 0;
  private request?: AbortController;

  constructor(
    private verify: VerifyEmail,
    private currentUserId: () => string | undefined,
  ) {}

  initialize(hash: string) {
    this.invalidate();
    this.done = false;
    this.token = securityToken(hash);
    this.error = this.token ? '' : '链接无效或已失效，请重新申请';
  }

  invalidate() {
    this.generation++;
    this.request?.abort();
    this.request = undefined;
    this.pending = false;
    this.error = '';
  }

  dispose() {
    this.invalidate();
    this.token = '';
  }

  async confirm(): Promise<void> {
    const userId = this.currentUserId();
    if (!this.token || !userId || this.pending) return;
    const generation = ++this.generation;
    const request = new AbortController();
    this.request = request;
    this.pending = true;
    this.error = '';

    try {
      await this.verify(this.token, request.signal);
      if (generation === this.generation && userId === this.currentUserId()) {
        this.done = true;
        this.token = '';
      }
    } catch (error) {
      if (generation === this.generation && userId === this.currentUserId()) {
        this.error =
          error instanceof Error ? error.message : '验证失败，请重试';
      }
    } finally {
      if (generation === this.generation) {
        this.pending = false;
        this.request = undefined;
      }
    }
  }
}
