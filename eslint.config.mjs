import antfu from '@antfu/eslint-config'
import withNuxt from './.nuxt/eslint.config.mjs'
import rules from './src/utils/eslint-config.mjs'

export default withNuxt(
  // // Your custom configs here
  antfu({
    typescript: {
      tsconfigPath: './tsconfig.json',
    },
    ignores: [
      'src-tauri/gen/**',
      'src-tauri/target/**',
      'src/bindings.ts',
    ],
    vue: true,
    formatters: true,
    rules,
  }),
)
