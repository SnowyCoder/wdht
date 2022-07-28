import { reactive } from "vue";
import { Socket } from "./socket/Socket";
import { Message } from "./storage";

export class MessageRegistry {
    messages: Array<Message> = reactive([]);

    private nextId = 0;
    private socket: Socket;

    constructor(socket: Socket) {
        this.socket = socket;
        const selfId = socket.dht.id;
        socket.plugins.name.events.on('name_change', (peerId, newName, oldName) => {
            let message;
            if (selfId == peerId) {
                message = 'Your name is now ' + newName;
            } else if (oldName === undefined) {
                message = newName + ' connected';
            } else if (newName === undefined) {
                message = oldName + ' disconnected';
            } else {
                message = oldName + ' has changed name into ' + newName;
            }
            this.postSystemMessage(message);
        });
        socket.events.on('p/message', (packet, sender) => {
            this.messages.push({
                id: this.nextId++,
                sender: this.socket.plugins.name.peerNames.value.get(sender) ?? 'unknown',
                message: packet.mex,
                time: new Date(),
            })
        });

        this.postSystemMessage('Your name is ' + this.socket.plugins.name.name.value + ' change it with \\name <your new name>');
    }

    postCommand(command: string) {
        const cmd = command.split(' ', 1)[0];
        const reminder = command.substring(cmd.length + 1);
        if (cmd == 'name') {
            const name = reminder.trim().replace('\n', ' ');
            if (name.length > 40) {
                this.postSystemMessage("Select a shorter name")
            } else {
                this.socket.plugins.name.name.value = name;
            }
        } else {
            this.postSystemMessage("Command not found");
        }
    }

    postMessage(text: string) {
        if (text.length == 0) return;
        if (text[0] == '/' || text[0] == '\\') {
            this.postCommand(text.substring(1));
            return;
        }

        this.messages.push({
            id: this.nextId++,
            sender: 'You',
            message: text,
            time: new Date(),
        });
        this.socket.sendPacket({
            type: 'message',
            mex: text,
        });
    }

    postSystemMessage(text: string) {
        this.messages.push({
            id: this.nextId++,
            message: text,
        });
    }


}
