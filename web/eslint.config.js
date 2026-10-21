import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import prettier from 'eslint-config-prettier';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';

export default defineConfig(
  {
    ignores: [
      'node_modules/**',
      '.svelte-kit/**',
      'build/**',
      'playwright-report/**',
      'test-results*/**',
      // Test harnesses retain their separate type/format/behavior gates.
      'tests/**',
      'e2e/**',
      'src/lib/api-client/schema.d.ts',
    ],
  },
  js.configs.recommended,
  ts.configs.recommended,
  svelte.configs.recommended,
  prettier,
  svelte.configs.prettier,
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.node,
        ...globals.serviceworker,
      },
    },
    rules: {
      curly: ['error', 'multi-line'],
      // This deployment serves at the origin root; navigation URLs are already
      // application paths or validated return destinations, not base-relative URLs.
      'svelte/no-navigation-without-resolve': 'off',
      // Lifecycle maps/sets hold timers, deduplication and observers. Making those
      // reactive would subscribe teardown effects to their own bookkeeping.
      'svelte/prefer-svelte-reactivity': 'off',
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' },
      ],
    },
  },
  {
    files: ['**/*.svelte', '**/*.svelte.ts'],
    languageOptions: {
      parserOptions: { parser: ts.parser },
    },
  },
);
