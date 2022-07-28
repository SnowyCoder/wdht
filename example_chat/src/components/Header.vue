<script setup lang="ts">
import ThemeSelector from './ThemeSelector.vue';
</script>

<template>
<div class="w-full bg-white dark:bg-slate-900">
  <div class="max-w-7xl mx-auto px-4 sm:px-6">
    <div class="flex justify-between items-center border-b-2 border-gray-100 dark:border-gray-700 py-1 md:justify-start md:space-x-10">

      <div class="flex justify-start lg:w-0 lg:flex-1">
        <a href="/">
          <span class="text-3xl dark:text-white">WebDHT Chat</span>
        </a>
      </div>

      <ThemeSelector class="h-full"/>
    </div>
  </div>
</div>
</template>
