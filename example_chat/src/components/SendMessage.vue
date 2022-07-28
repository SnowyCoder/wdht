<script setup lang="ts">import { computed, ref } from 'vue';

const emit = defineEmits<{
  (event: 'message', mex: string): void
}>();

const message = ref("");
const sendButton = ref<HTMLButtonElement>();

function onSend() {
  const text = message.value;
  if (text == null || text === '') return;
  message.value = '';

  emit('message', text);
  const el = sendButton.value;
  if (el) {
    el.classList.add('btn-send-mail-anim');
    setTimeout(() => el.classList.remove('btn-send-mail-anim'), 1000)
  }
}

function onKeydown(ev: KeyboardEvent) {
  if (ev.key === 'Enter' && !ev.shiftKey) {
    ev.preventDefault();
    onSend();
  }
}

const isSendDisabled = computed(() => message.value.length == 0);

</script>

<template>
  <div class="w-full h-full flex flex-row dark:bg-slate-800 bg-gray-100">
    <div class="text-div flex-grow">
      <input type="text" v-model="message"
          @keydown="onKeydown($event)"
          placeholder="Write message..."
          class="text-input">
      <span class="text-border"></span>
    </div>
    <button class="send-btn" :disabled="isSendDisabled" @click="onSend">
      <svg viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1" class="send-svg">
        <path class="send-path" ref="sendButton" stroke-linecap="round" pathLength="1" stroke-linejoin="round" d="M3 19v-8.93a2 2 0 01.89-1.664l7-4.666a2 2 0 012.22 0l7 4.666A2 2 0 0121 10.07V19M3 19a2 2 0 002 2h14a2 2 0 002-2M3 19l6.75-4.5M21 19l-6.75-4.5M3 10l6.75 4.5M21 10l-6.75 4.5m0 0l-1.14.76a2 2 0 01-2.22 0l-1.14-.76" />
      </svg>
    </button>
  </div>
</template>

<style scoped lang="scss">
.text-div {
  position: relative;
  width: 100%;
  height: 100%;
}
.text-border {
  position: absolute;
  bottom: 0;
  left: 0;
  width: 0;
  height: 1px;
  border: 1px solid transparent;
  transition: width 0.2s, border 0s 0.2s;
}

.text-input {
  @apply dark:text-white;
  height: 100%;
  width: 100%;
  background-color: transparent;
  padding: 3px 5px;
  margin: 0;
  border-bottom: solid 1px #eeeeee;
  outline: none;

  &:focus ~ .text-border {
    border-color: #4285F4;
    width: 100%;
    transition: width 0.3s;
  }
}

.send-btn {
  @apply enabled:(bg-blue-400 hover:bg-blue-500 dark:(bg-blue-800 hover:bg-blue-900));
  aspect-ratio: 1 / 1;
  transition: background-color 0.2s;
  path {
    transition: 0.3s;
    stroke-dasharray: 1;
  }
  &:hover:enabled path {
    d: path("M3 19v-8.93a2 2 0 012-2l7-0a2 0 0 012.22 0 l5 0A2 2 0 0121 10.07    V19M3 19a2 2 0 002 2h14a2 2 0 002-2M3 19l6.75-4.5M21 19l-6.75-4.5M3 10l6.75 4.5M21 10l-6.75 4.5m0 0l-1.14.76a2 2 0 01-2.22 0l-1.14-.76");
    transform: translate(0, -10%);
  }
}

.btn-send-mail-anim {
  animation: send-mail 1s linear;
}

@keyframes send-mail {
  20% {
    transform: translate(0, -100%);
  }
  21% {
    stroke-dashoffset: 1;
  }
  22% {
    transform: translate(0);
    stroke-dashoffset: 1;
  }
  100% {
    stroke-dashoffset: 0;
  }
}
</style>
