import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';

export default ts.config(
  { ignores: ['dist', 'node_modules'] },
  ...ts.configs.recommended,
  ...svelte.configs.recommended,
  {
    languageOptions: { globals: globals.browser },
  },
  {
    files: ['**/*.svelte', '**/*.svelte.ts'],
    languageOptions: { parserOptions: { parser: ts.parser, extraFileExtensions: ['.svelte'] } },
  },
  {
    rules: {
      // Names, titles and file names come from other people: never render them as HTML.
      'svelte/no-at-html-tags': 'error',
    },
  },
);
