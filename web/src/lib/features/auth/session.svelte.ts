import type { ApiClient, components } from '../../api-client';
import { api, ApiFailure } from '../../api-client';
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
    client.use({
      onResponse: ({ response }) => {
        if (response.status === 401) {
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
        this.user = user;
        this.error = '';
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

  onReset(callback: () => void) {
    this.resetHandlers.add(callback);
    return () => {
      this.resetHandlers.delete(callback);
    };
  }
}

export const auth = new SessionState();
