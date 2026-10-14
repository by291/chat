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
    changePassword: async (
      input: components['schemas']['PasswordChangeInput'],
    ): Promise<void> => {
      await unwrap(client.POST('/api/v1/auth/password', { body: input }));
    },
    emailStatus: (signal?: AbortSignal) =>
      unwrap(client.GET('/api/v1/auth/email', { signal })),
    enrollEmail: async (
      input: components['schemas']['EmailEnrollmentInput'],
    ) => {
      await unwrap(client.POST('/api/v1/auth/email', { body: input }));
    },
    verifyEmail: (token: string, signal?: AbortSignal) =>
      unwrap(
        client.POST('/api/v1/auth/email/verify', { body: { token }, signal }),
      ),
    requestRecovery: async (
      email: string,
      signal?: AbortSignal,
    ): Promise<void> => {
      await unwrap(
        client.POST('/api/v1/auth/password/recovery', {
          body: { email },
          signal,
        }),
      );
    },
    resetPassword: async (
      input: components['schemas']['PasswordResetInput'],
      signal?: AbortSignal,
    ): Promise<void> => {
      await unwrap(
        client.POST('/api/v1/auth/password/reset', { body: input, signal }),
      );
    },
  };
}
