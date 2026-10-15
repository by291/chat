/** Shared cookie-authenticated transport. Feature adapters own endpoint calls. */
import createClient from 'openapi-fetch';

import type { paths } from './schema';

export function createApiClient(
  options: {
    baseUrl?: string;
    fetch?: typeof fetch;
  } = {},
) {
  return createClient<paths>({
    baseUrl: options.baseUrl ?? '',
    fetch: options.fetch,
    credentials: 'include',
  });
}

export type ApiClient = ReturnType<typeof createApiClient>;
