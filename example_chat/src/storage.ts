import { reactive } from "vue";

export interface Message {
    id: number;// Local-only message identifier
    sender?: string;
    message: string;
    time?: Date;
}

export const messages = reactive<Array<Message>>([]);
