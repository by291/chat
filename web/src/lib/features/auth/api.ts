import { api, unwrap, type ApiClient, type components } from '../../api-client';

/** Authentication endpoints are independent of reactive session ownership. */
export function createAuthApi(client: ApiClient = api) {
  return {
    currentUser: () => unwrap(client.GET('/api/v1/auth/me')),
    login: (input: components['schemas']['LoginInput']) =>
      unwrap(client.POST('/api/v1/auth/login', { body: input })),
    register: (input: components['schemas']['RegisterInput']) =>
      unwrap(client.POST('/api/v1/auth/register', { body: input })),
    logout: () => unwrap(client.POST('/api/v1/auth/logout')),
  };
}
