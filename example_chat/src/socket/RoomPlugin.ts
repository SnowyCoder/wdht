import { ref, Ref, watch } from "vue";
import { Topic } from "web-dht";

import { Socket } from "./Socket";

const ROOM_PREFIX = 'dev.rossilorenzo.wdht.examplechat.';

interface RawTopic {
    type: 'topic' | 'raw_id',
    key: string,
}

export class RoomPlugin {
    private readonly socket: Socket;
    private dhtRefresh: any;
    private lastJoinRoom?: Promise<void>;

    private _room?: RawTopic;

    locHash: Ref<string> = ref("");

    constructor(socket: Socket) {
        this.socket = socket;

        watch(this.locHash, (val, old) => {
            location.hash = val ?? '';
            if (val == null || val.length == 0) {
                this.joinRoom();
                return;
            }
            if (val[0] == 't') {
                this.joinRoom({
                    type: 'topic',
                    key: val.substring(1),
                });
            } else if(val[0] == 'r') {
                this.joinRoom({
                    type: 'raw_id',
                    key: val.substring(1),
                })
            } else {
                console.warn("Unknown location.hash found: ", val);
            }
        });

        window.addEventListener('hashchange', () => {
            this.locHash.value = location.hash.substring(1);
        });
        this.locHash.value = location.hash.substring(1);
    }

    async joinRoom(room?: Topic) {
        if (this.lastJoinRoom) {
            this.lastJoinRoom = this.lastJoinRoom.then(() => this.joinRoom0(room));
        } else {
            this.lastJoinRoom = this.joinRoom0(room);
        }
        return this.lastJoinRoom;
    }

    private async joinRoom0(room?: Topic) {
        let raw: RawTopic | undefined;
        if (typeof room === 'string') {
            raw = {
                type: 'topic',
                key: room
            };
        } else {
            raw = room;
        }
        if (this._room?.key == raw?.key && this._room?.type == raw?.type) {
            this.lastJoinRoom = undefined;
            return;
        }

        if (this._room !== undefined) {
            await this.socket.dht.remove(this._room);
            for (const peer of this.socket.peers.values()) {
                peer.connection.close();
            }
            clearInterval(this.dhtRefresh)
        }
        if (raw === undefined) {
            this._room = undefined;
            this.lastJoinRoom = undefined;
            this.locHash.value = '';
            return;
        }
        this._room = raw;
        const originalKey = raw.key;
        if (raw.type == 'topic') {
            raw.key = ROOM_PREFIX + raw.key;
        }
        // 10 minutes
        await this.socket.dht.insert(raw, 10 * 64);
        // Refresh every 5 minutes
        this.dhtRefresh = setInterval(async () => {
            await this.socket.dht.insert(raw!, 10 * 64);
        }, 5 * 64 * 1000);
        const entries = await this.socket.dht.query(raw, 100);
        const selfId = this.socket.dht.id;
        for (const entry of entries) {
            if (entry.publisher == selfId) continue;
            this.socket.connectTo(entry.publisher)
                .catch(err => console.warn("Failed to connect to ", entry.publisher, err));
        }
        this.lastJoinRoom = undefined;
        this.locHash.value = raw.type[0] + originalKey;
    }
}
