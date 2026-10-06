import js from '@eslint/js'
import eslintConfigPrettier from 'eslint-config-prettier'
import globals from 'globals'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  {
    ignores: [
      'dist/**',
      'src-tauri/**',
      'node_modules/**',
      '.pnpm-store/**',
      'target/**',
      'crates/**',
      'docs/book/**'
    ]
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2020,
      globals: globals.browser
    },
    rules: {
      // Solid reads a signal as a statement to subscribe, and assigns DOM refs
      // by compiling `ref={name}` against a `let` that has no other write.
      '@typescript-eslint/no-unused-expressions': 'off',
      'no-unassigned-vars': 'off'
    }
  },
  {
    files: ['scripts/**/*.{js,mjs,cjs}'],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.node
    }
  },
  // Disable ESLint rules that conflict with Prettier formatting
  eslintConfigPrettier
)
