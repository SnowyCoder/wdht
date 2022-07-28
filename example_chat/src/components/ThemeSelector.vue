<script setup lang="ts">
import { DesktopComputerIcon, MoonIcon, SunIcon } from '@heroicons/vue/outline';
import { computed, ref } from 'vue';

const THEMES = ['dark', 'light', 'system'] as const;
type Theme = typeof THEMES[number];

const currentTheme = ref('system' as Theme);

if (localStorage.theme == 'dark') {
  currentTheme.value = 'dark';
} else if (localStorage.theme == 'light') {
  currentTheme.value = 'light';
}

function switchTheme() {
  const index = (THEMES.indexOf(currentTheme.value) + 1) % THEMES.length;
  const theme = THEMES[index];
  currentTheme.value = theme;
  if (theme != 'system') {
    localStorage.theme = theme;
  } else {
    localStorage.removeItem('theme');
  }

  if (theme == 'dark' || (theme == 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)) {
    document.documentElement.classList.add('dark');
  } else if (theme == 'light') {
    document.documentElement.classList.remove('dark');
  }
}

const icon = computed(() => {
  const theme = currentTheme.value;
  if (theme == 'dark') return MoonIcon;
  if (theme == 'light') return SunIcon;
  return DesktopComputerIcon;
});

const currentTitle = computed(() => {
  const theme = currentTheme.value;
  return 'Current theme: ' + (theme ?? 'system');
})

</script>

<template>
<button type="button" class="dark:text-white" @click="switchTheme" :title="currentTitle">
  <component :is="icon" class="h-10 w-10"/>
</button>
</template>
