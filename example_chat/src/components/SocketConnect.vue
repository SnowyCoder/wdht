<script setup lang="ts">
import { ref, shallowRef } from 'vue';
import { Socket } from '../socket/Socket';
import App from './App.vue';

let isLoading = ref(true);
let isConnected = ref(false);

const socketRef = shallowRef<Socket | null>(null);
const loadingState = ref("Loading...");

async function loadDht() {
  const socket = await Socket.create({
    progress: (p) => {
      switch (p) {
      case 'download': loadingState.value = 'Downloading...'; break;
      case 'bootstrap': loadingState.value = 'Bootstrapping DHT...'; break;
    }},
  });
  socketRef.value = socket;

  if (socket.isOnline) {
    isConnected.value = true;
  }
  isLoading.value = false;
}

loadDht();
</script>

<template>
  <div class="flex flex-col justify-center items-center dark:text-white" v-if="isLoading || !isConnected">
    <p class="text-5xl text-center py-4">WebDHT Chat</p>

    <p class="text-xl" v-if="isLoading">{{loadingState}}</p>
    <p class="text-xl" v-else-if="!isConnected">Connection failed, try again later</p>
  </div>
  <App v-else :socket="socketRef!"></App>
</template>

<style scoped>
</style>
