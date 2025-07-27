import { fileURLToPath } from 'node:url'

// https://nuxt.com/docs/api/configuration/nuxt-config
export default defineNuxtConfig({
  compatibilityDate: '2025-07-26',
  devtools: { enabled: true },
  ssr: false,

  eslint: {
    config: {
      standalone: false,
    },
  },

  typescript: {
    typeCheck: true,
  },

  srcDir: 'src/',

  modules: [
    '@nuxt/eslint',
    '@nuxt/icon',
    '@nuxt/image',
    '@nuxt/scripts',
    '@nuxt/fonts',
    '@una-ui/nuxt',
    '@vueuse/nuxt',
    '@unocss/nuxt',
    '@pinia/nuxt',
  ],

  nitro: {
    publicAssets: [
      {
        dir: fileURLToPath(new URL('./src/public', import.meta.url)),
      },
    ],
  },

  unocss: {
    nuxtLayers: true,
  },

  una: {
    prefix: 'N',
    themeable: true,
    global: true,
  },
})
