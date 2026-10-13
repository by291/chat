/** Normalize typed HTTP failures into the stable user-facing error contract. */
export class ApiFailure extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
    this.name = 'ApiFailure';
  }
}

export async function unwrap<T>(
  result: Promise<{
    data?: T;
    error?: unknown;
    response: Response;
  }>,
): Promise<T> {
  let response;

  try {
    response = await result;
  } catch {
    throw new ApiFailure(0, 'network_error', '连接失败，请检查网络后重试');
  }
  if (!response.response.ok) {
    const body = response.error as
      | {
          code?: string;
          message?: string;
        }
      | undefined;
    throw new ApiFailure(
      response.response.status,
      body?.code ?? 'request_failed',
      body?.message ?? '请求失败，请稍后重试',
    );
  }
  return response.data as T;
}
