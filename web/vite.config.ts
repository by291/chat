import { sveltekit } from '@sveltejs/kit/vite';
import adapter from '@sveltejs/adapter-static';
import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const previewDirectory = process.env.CHAT_TEST_WEB_BUILD_DIR
  ? path.resolve(process.env.CHAT_TEST_WEB_BUILD_DIR)
  : undefined;
if (previewDirectory) {
  const relative = path.relative(
    path.join(repositoryRoot, '.local/tests'),
    previewDirectory,
  );
  if (
    path.isAbsolute(relative) ||
    !/^run-[\w-]+[/\\]web-preview-[\w-]+$/.test(relative)
  ) {
    throw new Error('Preview build directory must belong to an owned test run');
  }
}

const proxy = {
  '/api': process.env.CHAT_API_PROXY ?? 'http://127.0.0.1:8080',
  '/connection': {
    target: process.env.CHAT_CENTRIFUGO_PROXY ?? 'ws://127.0.0.1:8000',
    ws: true,
  },
};

const mediaOrigin = process.env.CHAT_MEDIA_ORIGIN ?? 'http://localhost:8336';
const parsedMediaOrigin = new URL(mediaOrigin);
if (
  !['http:', 'https:'].includes(parsedMediaOrigin.protocol) ||
  parsedMediaOrigin.origin !== mediaOrigin
) {
  throw new Error('CHAT_MEDIA_ORIGIN must be an exact HTTP(S) origin');
}
// Runtime URL validation above narrows this environment value to a CSP host source.
const cspMediaOrigin =
  mediaOrigin as `${'http' | 'https'}://${string}.${string}`;
export default defineConfig({
  plugins: [
    tailwindcss(),
    sveltekit({
      ...(previewDirectory
        ? {
            outDir: path.join(previewDirectory, 'kit'),
            version: { name: path.basename(previewDirectory) },
          }
        : {}),
      adapter: adapter({
        fallback: 'index.html',
        ...(previewDirectory
          ? {
              pages: path.join(previewDirectory, 'site'),
              assets: path.join(previewDirectory, 'site'),
            }
          : {}),
      }),
      csp: {
        mode: 'hash',
        directives: {
          'default-src': ['self'],
          'script-src': ['self'],
          'style-src': ['self', 'unsafe-inline'],
          'img-src': ['self', 'data:', 'blob:', cspMediaOrigin],
          'connect-src': ['self', cspMediaOrigin],
          'object-src': ['none'],
          'base-uri': ['self'],
          'form-action': ['self'],
        },
      },
    }),
  ],
  server: {
    proxy,
  },
  preview: { proxy },
});
