<script lang="ts">
export const SOCKET_KEY = Symbol() as InjectionKey<Socket>;
</script>

<script setup lang="ts">
import { InjectionKey, provide } from 'vue';
import { Socket } from '@/socket/Socket';
import Chat from './Chat.vue';
import { MessageRegistry } from '@/MessageRegistry';
import ChooseRoom from './ChooseRoom.vue';
import Header from './Header.vue';

const {
  socket
} = defineProps<{
  socket: Socket
}>();

const room = socket.plugins.room.locHash;

provide(SOCKET_KEY, socket);
const registry = new MessageRegistry(socket);
</script>

<template>
  <div class="flex flex-col h-full w-full fixed items-center">
    <Header></Header>
    <div class="min-h-1 h-full w-full sm:w-2/3">
      <Chat :registry="registry" v-if="room"></Chat>
      <ChooseRoom v-else></ChooseRoom>
    </div>
  </div>
</template>
