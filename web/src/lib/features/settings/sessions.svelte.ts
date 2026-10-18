import {
  MutationObserver,
  QueryObserver,
  type MutationObserverResult,
  type QueryClient,
  type QueryObserverResult,
} from '@tanstack/svelte-query';

import type { components } from '#lib/api-client';

import { settingsApi } from './api';

type Session = components['schemas']['SessionView'];
type SessionRequests = Pick<typeof settingsApi, 'sessions' | 'revokeSession'>;

/** Observes device records and revoke feedback for one authenticated settings lifetime. */
export class SettingsSessions {
  // Observer results reference the authoritative query cache; no editable list is mirrored.
  private queryState = $state.raw<QueryObserverResult<Session[]> | undefined>();
  private revokeState = $state.raw<
    MutationObserverResult<void, Error, string> | undefined
  >();
  private active = true;
  private queryKey: readonly ['settings', 'sessions', string];
  private query: QueryObserver<Session[]>;
  private mutation: MutationObserver<void, Error, string>;
  private unsubscribeQuery: () => void;
  private unsubscribeMutation: () => void;

  constructor(
    private options: {
      queryClient: QueryClient;
      userId: string;
      api?: SessionRequests;
    },
  ) {
    const requests = options.api ?? settingsApi;
    this.queryKey = ['settings', 'sessions', options.userId];
    this.query = new QueryObserver<Session[]>(options.queryClient, {
      queryKey: this.queryKey,
      queryFn: ({ signal }) => requests.sessions(signal),
      // Loading remains an explicit settings action, including offline attempts.
      enabled: false,
      networkMode: 'always',
      retry: false,
    });
    this.queryState = this.query.getCurrentResult();
    this.unsubscribeQuery = this.query.subscribe((result) => {
      this.queryState = result;
    });

    this.mutation = new MutationObserver<void, Error, string>(
      options.queryClient,
      {
        mutationFn: (sessionId: string) => requests.revokeSession(sessionId),
        networkMode: 'always',
        retry: false,
        gcTime: 0,
        onSuccess: async () => {
          // A revoke may commit after navigation/logout. Only the live owner reloads;
          // revoking this device retains the transport's existing 401/reset behavior.
          if (this.active) await this.load();
        },
      },
    );
    this.revokeState = this.mutation.getCurrentResult();
    this.unsubscribeMutation = this.mutation.subscribe((result) => {
      this.revokeState = result;
    });
  }

  get sessions(): Session[] {
    return this.active ? (this.queryState?.data ?? []) : [];
  }

  get pending(): string | null {
    return this.active && this.revokeState?.isPending
      ? (this.revokeState.variables ?? null)
      : null;
  }

  get error(): string {
    if (!this.active || this.revokeState?.isPending) return '';
    if (this.revokeState?.isError) {
      return this.revokeState.error instanceof Error
        ? this.revokeState.error.message
        : '撤销失败';
    }
    if (this.queryState?.isError) {
      return this.queryState.error instanceof Error
        ? this.queryState.error.message
        : '加载失败';
    }
    return '';
  }

  async load() {
    if (!this.active) return;

    // Query cancellation fences late transport resolutions as well as aborting I/O.
    await this.options.queryClient.cancelQueries({
      queryKey: this.queryKey,
      exact: true,
    });
    if (this.active) await this.query.refetch();
  }

  async revoke(sessionId: string) {
    if (!this.active || this.pending) return;

    try {
      await this.mutation.mutate(sessionId);
    } catch {
      // The mutation observer exposes durable failure feedback through error.
    }
  }

  /** Reset and route cleanup may both stop this owner; neither may remove a successor's cache. */
  stop() {
    if (!this.active) return;

    this.active = false;
    this.unsubscribeQuery();
    this.unsubscribeMutation();
    this.mutation.reset();
    this.options.queryClient.removeQueries({
      queryKey: this.queryKey,
      exact: true,
    });
  }
}
