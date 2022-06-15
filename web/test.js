// This file is a stub client that tries to connect to a native node but performs
// no handshake, this can be used instead of the actual implementation to search for
// browser-induced non-deterministic WebRTC bugs (looking at you firefox).
const SERVER_URL = 'http://localhost:3141';

const ret = new RTCPeerConnection({
    iceServers: [{ urls: ["stun:stun1.l.google.com:19302"] }],
});
// This can be also used in the generated wasm .js file to inspect what
// is being done to a js object.
const conn = new Proxy(ret, {
    get(target, propKey, receiver) {
        const targetValue = Reflect.get(target, propKey, ret);
        if (typeof targetValue === 'function') {
            return function (...args) {
                console.log('CALL', propKey, args);
                return targetValue.apply(ret, args);
            }
        } else {
            console.log('GET', propKey);
            return targetValue;
        }
    },
    set(target, propKey, val) {
        console.log('SET', propKey, val);
        return Reflect.set(target, propKey, val);
    },
});

conn.onicecandidate = async c => {
    if (!c.candidate) {
        let desc = conn.localDescription;
        console.log(desc);
        let data = {
            id: 'f5a7d2ab3bea66eaf40f0ca21c07c69d85015e04',
            offer: desc,
        };
        let res = await fetch(SERVER_URL, {
            method: 'POST',
            mode: 'cors',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify(data),
        }).then(x => x.json());
        console.log(res);
        conn.setRemoteDescription(res.answer);
    }
};

conn.createDataChannel('wdht', { id: 0, protocol: 'wrtc_json', negotiated: true });
conn.createOffer().then(x => conn.setLocalDescription(x));
