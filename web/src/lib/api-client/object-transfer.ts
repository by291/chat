/** Send an exact signed PUT without cookies; the browser owns Content-Length and Host. */
export function uploadObject(
  grant: { url: string; headers: Record<string, string> },
  file: Blob,
  options: { signal: AbortSignal; progress: (percent: number) => void },
): Promise<void> {
  return new Promise((resolve, reject) => {
    const request = new XMLHttpRequest();
    const abort = () => request.abort();
    const finish = (error?: Error) => {
      options.signal.removeEventListener('abort', abort);
      request.onload = null;
      request.onerror = null;
      request.onabort = null;
      request.upload.onprogress = null;
      if (error) {
        reject(error);
      } else {
        resolve();
      }
    };

    if (options.signal.aborted) {
      reject(new DOMException('Upload canceled', 'AbortError'));
      return;
    }

    request.open('PUT', grant.url);
    request.withCredentials = false;
    for (const [name, value] of Object.entries(grant.headers)) {
      if (!['host', 'content-length'].includes(name.toLowerCase())) {
        request.setRequestHeader(name, value);
      }
    }

    request.upload.onprogress = (event) => {
      if (event.lengthComputable) {
        options.progress(
          Math.min(100, Math.round((event.loaded / event.total) * 100)),
        );
      }
    };
    request.onload = () =>
      finish(
        request.status >= 200 && request.status < 300
          ? undefined
          : new Error(`上传失败（${request.status}）`),
      );
    request.onerror = () => finish(new Error('上传连接失败，请重试'));
    request.onabort = () =>
      finish(new DOMException('Upload canceled', 'AbortError'));

    options.signal.addEventListener('abort', abort, { once: true });
    request.send(file);
  });
}
