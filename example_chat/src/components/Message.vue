<script setup lang="ts">
import { Message } from '@/storage';
import { toRef } from 'vue';

const props = defineProps<{
    message: Message,
}>();
const message = toRef(props, 'message');

//const isSystem = computed(() => !message.value.sender);

function formatDate(d: Date) {
    const now = new Date();
    const p = (x: any) => ('0' + x).slice(-2);
    const time = p(d.getHours()) + ':' + p(d.getMinutes());
    if (d.getFullYear() == now.getFullYear() && d.getMonth() == now.getMonth() && d.getDay() == now.getDay()) {
        return time;
    }
    const day = d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate());
    return day + ' ' + time;
}

</script>


<template>
<div class="flex flex-col md:max-w-100 w-fit text-left bg-white dark:bg-slate-800 rounded-2xl p-3">
    <p class="text-sky-800 dark:text-sky-400 pb-1" v-if="message.sender">
        {{message.sender}}
    </p>
    <div class="flex flex-row flex-wrap justify-end min-w-30">
        <div class="whitespace-pre-wrap flex-grow dark:text-white" style="word-break: break-word;">{{message.message}}</div>
        <p class="text-right pl-2 text-gray-700 dark:text-gray-400 text-sm" v-if="message.time">
            {{formatDate(message.time)}}
        </p>
    </div>
</div>
</template>
