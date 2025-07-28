<script setup lang="ts">
import type { NDropdownMenuItemProps } from '#una/types'
import { useAppInfo } from '~/api/meta'

const { data } = useAppInfo()
const label = computed(() => {
  if (data.value === undefined) {
    return 'Mado'
  }
  return `${data.value.name} ${data.value.version}`
})

const router = useRouter()

const items: NDropdownMenuItemProps[] = [
  {
    label: 'Home',
    shortcut: 'Ctrl+H',
    /**
     *
     */
    onSelect: () => {
      void router.replace('/')
    },
  },
]
</script>

<template>
  <NDropdownMenu
    :items
    btn="ghost"
    class="font-bold"
    :label
  />
</template>
