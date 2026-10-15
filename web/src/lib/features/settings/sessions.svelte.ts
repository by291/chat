import type { QueryClient } from '@tanstack/svelte-query';

import type { components } from '#lib/api-client';

import { settingsApi } from './api';

type Session = components['schemas']['SessionView'];
type SessionRequests = Pick<typeof settingsApi, 'sessions' | 'revokeSession'>;

/** Owns session-list cache writes and revoke feedback for one authenticated settings lifetime. */
export class SettingsSessions {
  pending = $state<string | null>(null);
  error = $state('');

  private revision = $state(0);
  private active = true;
  private requestGeneration = 0;
  private request?: AbortController;
  private queryKey: readonly ['settings', 'sessions', string];
  private requests: SessionRequests;
  private unsubscribeCache: () => void;

  constructor(
    private options: {
      queryClient: QueryClient;
      userId: string;
      api?: SessionRequests;
    },
  ) {
    this.queryKey = ['settings', 'sessions', options.userId];
    this.requests = options.api ?? settingsApi;
    this.unsubscribeCache = options.queryClient
      .getQueryCache()
      .subscribe(() => {
        this.revision++;
      });
  }

  get sessions(): Session[] {
    // Cache notifications drive Svelte consumers; the owner keeps no second list.
    this.revision;
    if (!this.active) return [];

    return (
      this.options.queryClient.getQueryData<Session[]>(this.queryKey) ?? []
    );
  }

  async load() {
    if (!this.active) return;

    this.request?.abort();
    const request = new AbortController();
    const generation = ++this.requestGeneration;
    this.request = request;

    try {
      const sessions = await this.requests.sessions(request.signal);
      // Aborts save work, while lifetime/generation fences protect a cleared cache
      // even when the transport still resolves after logout or route destruction.
      if (!this.current(generation)) return;

      this.options.queryClient.setQueryData(this.queryKey, sessions);
    } catch (reason) {
      if (this.current(generation)) {
        this.error = reason instanceof Error ? reason.message : '加载失败';
      }
    } finally {
      if (this.request === request) this.request = undefined;
    }
  }

  /** A revoke can still commit after navigation; ended owners discard feedback and never reload. */
  async revoke(sessionId: string) {
    if (!this.active || this.pending) return;

    this.pending = sessionId;
    this.error = '';

    try {
      await this.requests.revokeSession(sessionId);
      if (!this.active) return;

      // Re-read after the durable revoke commits, including revoking this device.
      // The shared authenticated transport retains its existing 401/reset behavior.
      await this.load();
    } catch (reason) {
      if (this.active) {
        this.error = reason instanceof Error ? reason.message : '撤销失败';
      }
    } finally {
      if (this.active) this.pending = null;
    }
  }

  /** Reset and route cleanup may both stop this owner; neither may remove a successor's cache. */
  stop() {
    if (!this.active) return;

    this.active = false;
    this.requestGeneration++;
    this.request?.abort();
    this.request = undefined;

    this.options.queryClient.removeQueries({
      queryKey: this.queryKey,
      exact: true,
    });
    this.unsubscribeCache();
    this.pending = null;
  }

  private current(generation: number): boolean {
    return this.active && this.requestGeneration === generation;
  }
}
