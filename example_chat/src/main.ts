import { createApp } from 'vue';
import './style.css';
import 'virtual:windi.css'
import 'virtual:windi-devtools'
import SocketConnect from './components/SocketConnect.vue';

createApp(SocketConnect).mount('#app');
