<script setup lang="ts">

import { Socket } from '@/socket/Socket';
import { SOCKET_KEY } from './App.vue';
import { inject, ref } from 'vue';
import { Topic } from 'web-dht';

const socket = inject(SOCKET_KEY)!;
const isJoining = ref(false);
const name = ref("");

const emit = defineEmits(['joined']);

function joinRandom() {
  joinTopicRoom(Socket.createRandomTopic());
}

function joinName() {
  if (name.value == '') {
    joinRandom();
  } else {
    joinTopicRoom(name.value.toLowerCase());
  }
}

async function joinTopicRoom(topic: Topic) {
  isJoining.value = true;
  try {
    await socket.plugins.room.joinRoom(topic);
    emit('joined');
  } catch(e) {
    isJoining.value = false;
    alert(e);
  }
}

</script>

<template>

<div v-if="!isJoining" class="flex items-center justify-center flex-col md:flex-row py-4">
  <div class="w-64 flex h-12 flex-row">
    <input v-model="name" class="text-grey-900 h-full w-full rounded-l-md border-gray-200 bg-white px-2 text-lg text-gray-600 outline-none dark:border-gray-600 dark:bg-gray-700 dark:text-gray-100" type="text" placeholder="Room name" />
    <button @click="joinName" class="mx-0 h-full w-20 rounded-r-md bg-blue-500 px-2 font-bold text-white ring-0 hover:bg-blue-700">Join</button>
  </div>
  <p class="px-8 py-4 md:py-0 dark:text-white">or</p>
  <div class="w-64 flex justify-center md:(justify-start)">
    <button class="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded" @click="joinRandom">
      Join random
    </button>
  </div>
</div>
<div v-else>
  Joining room...
</div>

</template>
