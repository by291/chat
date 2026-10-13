import { sveltekit } from '@sveltejs/kit/vite';
import adapter from '@sveltejs/adapter-static';
import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';

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
      adapter: adapter({ fallback: 'index.html' }),
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
    proxy: {
      '/api': process.env.CHAT_API_PROXY ?? 'http://127.0.0.1:8080',
      '/connection': {
        target: process.env.CHAT_CENTRIFUGO_PROXY ?? 'ws://127.0.0.1:8000',
        ws: true,
      },
    },
  },
});
