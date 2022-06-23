<script setup lang="ts">
// This starter template is using Vue 3 <script setup> SFCs
// Check out https://vuejs.org/api/sfc-script-setup.html#script-setup
import HelloWorld from './components/HelloWorld.vue'

import init, {WebDht} from "web-dht";
import { ref } from 'vue';



let isLoading = ref(true);
let isConnected = ref(false);

async function loadDht() {
  await init();
  const dht = await WebDht.create(["http://127.0.0.1:3141"]);

  isLoading.value = false;

  if (dht.connection_count() == 0) {
    return;
  }
  isConnected.value = true;

}

loadDht()
</script>

<template>
  <h1 v-if="isLoading">Loading...</h1>
  <h1 v-else-if="!isConnected">Connection failed, try again later</h1>
  <HelloWorld v-else msg="Vite + Vue" />
</template>

<style scoped>
.logo {
  height: 6em;
  padding: 1.5em;
  will-change: filter;
}
.logo:hover {
  filter: drop-shadow(0 0 2em #646cffaa);
}
.logo.vue:hover {
  filter: drop-shadow(0 0 2em #42b883aa);
}
</style>
