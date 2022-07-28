import EventEmitter from "eventemitter3";
import { customRef, Ref, shallowRef, triggerRef } from "vue";

import { Socket } from "./Socket";

export interface NameEventTypes {
    // Peer changed name (peer_id, new_name, old_name)
    'name_change': [string, string | undefined, string | undefined],
}

export class NamePlugin {
    private readonly socket: Socket;
    private _name = "";
    readonly name: Ref<string>;
    readonly peerNames = shallowRef(new Map<string, string>());
    readonly events = new EventEmitter<NameEventTypes>();


    constructor(socket: Socket) {
        this.socket = socket;
        this.name = customRef((track, trigger) => {
            return {
                get: () => {
                    track();
                    return this._name;
                },
                set: (val: string) => {
                    const oldName = this._name;
                    this._name = val;
                    const selfId = this.socket.dht.id;
                    this.peerNames.value.set(selfId, val);
                    triggerRef(this.peerNames);
                    this.events.emit('name_change', selfId, val, oldName);
                    this.socket.sendPacket({
                        type: 'set-name',
                        name: val,
                    });
                    trigger();
                }
            }
        });
        this._name = beautifulRandomName();
        this.peerNames.value.set(this.socket.dht.id, this._name);

        socket.events.on('p/set-name', (packet, sender) => {
            const oldName = this.peerNames.value.get(sender);
            this.peerNames.value.set(sender, packet.name);
            triggerRef(this.peerNames);
            this.events.emit('name_change', sender, packet.name, oldName);
        });
        socket.events.on('connection_open', peerId => {
            this.socket.sendPacket({
                type: 'set-name',
                name: this._name,
            }, peerId);
        });
        socket.events.on('disconnect', peerId => {
            const oldName = this.peerNames.value.get(peerId);
            this.peerNames.value.delete(peerId);
            triggerRef(this.peerNames);
            this.events.emit('name_change', peerId, undefined, oldName);
        });
    }
}

function beautifulRandomName(): string {
    const adjectives = [
        'happy', 'angry', 'hungry', 'blushing', 'brave', 'cute', 'evil', 'foolish',
        'naughty', 'lucky', 'scary', 'super', 'zealous', 'tired', 'upset', 'sad', 'gloomy',
        'edgy', 'good', 'loyal', 'friendly', 'melancholy',
    ];
    let center = ' ';
    if (Math.random() < 0.001) center += 'smol ';
    const names = [
        'bard', 'dinosaur', 'wizard', 'warrior', 'artificier', 'barbarian', 'cleric',
        'druid', 'fighter', 'monk', 'paladin', 'ranger', 'rogue', 'sorcerer', 'warlock',
        'master', 'familiar',
    ];
    const adj = adjectives[Math.floor(Math.random() * adjectives.length)];
    const name = names[Math.floor(Math.random() * names.length)]
    return adj + center + name;
}
