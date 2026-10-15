import type { ApiClient, components } from '#lib/api-client';
import { api, ApiFailure } from '#lib/api-client';

import { createAuthApi } from './api';

/** Owns identity for the browser lifetime and fences responses from superseded sessions. */
export class SessionState {
  user = $state<components['schemas']['UserView'] | undefined>();
  pending = $state(true);
  error = $state('');

  private generation = 0;
  private resetHandlers = new Set<() => void>();
  private requests: ReturnType<typeof createAuthApi>;

  constructor(client: ApiClient = api) {
    this.requests = createAuthApi(client);

    const requestGenerations = new WeakMap<Request, number>();
    client.use({
      onRequest: ({ request }) => {
        requestGenerations.set(request, this.generation);
      },
      onResponse: ({ request, response }) => {
        // A revoked request from an older identity must not sign out the new
        // login. Weak keys also expire when a request fails without a response.
        if (
          response.status === 401 &&
          requestGenerations.get(request) === this.generation
        ) {
          this.reset();
        }
      },
    });
  }

  reset() {
    this.generation++;
    this.user = undefined;
    this.pending = false;

    for (const callback of this.resetHandlers) {
      callback();
    }
  }

  async restore() {
    const generation = this.generation;

    try {
      const user = await this.requests.currentUser();
      if (generation === this.generation) {
        // Cookie changes in another tab can restore a different account without
        // calling login here. Start a new lifetime, while same-user profile
        // revalidation keeps drafts, caches and outstanding requests intact.
        if (this.user?.id !== user.id) {
          if (this.user) {
            this.reset();
          } else {
            this.generation++;
          }
        }
        this.user = user;
        this.error = '';
        this.pending = false;
      }
    } catch (error) {
      if (
        error instanceof ApiFailure &&
        error.status !== 401 &&
        generation === this.generation
      ) {
        this.error = error.message;
      }
    } finally {
      if (generation === this.generation) {
        this.pending = false;
      }
    }
  }

  async login(input: components['schemas']['LoginInput']) {
    return this.authenticate(() => this.requests.login(input));
  }

  async register(input: components['schemas']['RegisterInput']) {
    return this.authenticate(() => this.requests.register(input));
  }

  private async authenticate(
    request: () => Promise<components['schemas']['UserView']>,
  ) {
    const generation = ++this.generation;
    this.pending = true;
    this.error = '';

    try {
      const user = await request();
      if (generation === this.generation) {
        this.user = user;
      }
      return user;
    } finally {
      if (generation === this.generation) {
        this.pending = false;
      }
    }
  }

  async logout() {
    await this.requests.logout();
    this.reset();
  }

  /** Revoke this lifetime only after the password change commits on the server. */
  async changePassword(
    input: components['schemas']['PasswordChangeInput'],
  ): Promise<void> {
    const generation = this.generation;

    await this.requests.changePassword(input);

    if (generation === this.generation) {
      this.reset();
    }
  }

  onReset(callback: () => void) {
    this.resetHandlers.add(callback);
    return () => {
      this.resetHandlers.delete(callback);
    };
  }
}

export const auth = new SessionState();
