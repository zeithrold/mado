<script setup lang="ts">
import { exit } from '@tauri-apps/plugin-process'

/**
 * Closes the application.
 */
function closeApp() {
  void exit(0)
}
</script>

<template>
  <NButton btn="ghost" icon @click="closeApp()">
    <Icon name="bx:x" :size="24" />
  </NButton>
</template>
