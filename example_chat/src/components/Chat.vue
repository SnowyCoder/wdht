<script setup lang="ts">
import { MessageRegistry } from '@/MessageRegistry';
import { Message } from '@/storage';
import MessageVue from './Message.vue';
import SendMessage from './SendMessage.vue';


const { registry } = defineProps<{
    registry: MessageRegistry,
}>();


function sendMessage(text: string) {
   registry.postMessage(text);
}

function messageClass(mex: Message): string[] {
  if (mex.sender === undefined) {
    return ['justify-center'];
  }
  if (mex.sender === 'You') {
    return ['justify-end'];
  }
  return [];
}

</script>

<template>
<div class="h-full w-full flex flex-col">
    <transition-group name="mex-list" tag="div" class="flex flex-col-reverse overflow-y-auto flex-grow p-2 rounded-t">
      <div v-for="message of registry.messages.slice().reverse()" :key="message.id" class="mex-list-item mb-2 w-full flex flex-row" :class="messageClass(message)">
        <MessageVue :message="message"></MessageVue>
      </div>
    </transition-group>
    <div class="h-10">
        <SendMessage @message="sendMessage"/>
    </div>
</div>

</template>

<style scoped>
.mex-list-item {
  transition: all 0.2s;
}
.mex-list-enter, .mex-list-enter-active {
  transform: translateY(100px);
}
.mex-list-enter-to {
  transform: translateY(0px);
}
</style>
