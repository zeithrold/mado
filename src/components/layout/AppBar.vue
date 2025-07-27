<script setup lang="ts">
import { useAppInfo } from '~/api/meta';

const { data } = useAppInfo()
const title = computed(() => {
  if (data.value === undefined) {
    return 'Mado'
  }
  return `${data.value.name} ${data.value.version}`
})
</script>

<template>
  <div data-tauri-drag-region class="w-full p-1">
    <NButton btn="ghost" class="font-bold">
      {{ title }}
    </NButton>
  </div>
</template>
