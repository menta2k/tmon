import pluginVue from 'eslint-plugin-vue'
import { withVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'
import skipFormatting from 'eslint-config-prettier'

export default withVueTs(
  {
    ignores: [
      'dist/**',
      'coverage/**',
      'node_modules/**',
      'src/api/schema.d.ts',
      'playwright-report/**',
      'test-results/**',
    ],
  },
  pluginVue.configs['flat/recommended'],
  vueTsConfigs.recommended,
  {
    rules: {
      'vue/multi-word-component-names': 'off',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
      'max-lines': ['warn', { max: 800, skipBlankLines: true, skipComments: true }],
      'max-depth': ['error', 4],
    },
  },
  {
    files: ['tests/**/*.ts'],
    rules: { 'vue/one-component-per-file': 'off' },
  },
  skipFormatting,
)
