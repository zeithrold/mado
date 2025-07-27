<script setup lang="ts">
const message = 'Hello, world!'
</script>

<template>
  <div>
    <h1 class="">
      {{ message }}
    </h1>
    <NButton>Hello, world!</NButton>
    <NuxtLink to="/hello">
      <NButton>Go to hello</NButton>
    </NuxtLink>
  </div>
</template>
