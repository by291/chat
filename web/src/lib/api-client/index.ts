import { createApiClient } from './client';

export { createApiClient, type ApiClient } from './client';

export { ApiFailure, unwrap } from './errors';

export type { paths, components } from './schema';

// SSR is disabled. This client is shared within one browser application instance.
export const api = createApiClient();
