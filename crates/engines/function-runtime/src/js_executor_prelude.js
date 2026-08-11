// Prelude installed at JS isolate boot. Defines the `molesignal` global the spec
// promises (fields / set / del / now / log / parse_json / encode_json / sha256).
// 注意：
// - 不引入 deno_runtime，所以 globalThis 上没有 fetch / setTimeout / Promise scheduler，
//   `crypto.subtle` 也缺；sha256 退化成 pure-JS 实现，避免每次落到 Rust op。
// - `__reset` / `__dump` 是内部 hook，Rust 侧每条 event 调用；用户不要碰。

globalThis.molesignal = (() => {
  let _fields = {};
  const _logs = [];

  // 32-bit pure-JS SHA-256（FIPS 180-4 参考实现，~200 行裁剪版）。
  const K = new Uint32Array([
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
    0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ]);
  const rotr = (x, n) => (x >>> n) | (x << (32 - n));
  function sha256(input) {
    const enc = new TextEncoder();
    const bytes = enc.encode(String(input));
    const bitLen = bytes.length * 8;
    // pad to multiple of 64 bytes; last 8 bytes hold bit length BE.
    const padLen = (bytes.length + 9 + 63) & ~63;
    const buf = new Uint8Array(padLen);
    buf.set(bytes);
    buf[bytes.length] = 0x80;
    const hi = Math.floor(bitLen / 0x100000000);
    const lo = bitLen >>> 0;
    buf[padLen - 8] = (hi >>> 24) & 0xff;
    buf[padLen - 7] = (hi >>> 16) & 0xff;
    buf[padLen - 6] = (hi >>> 8) & 0xff;
    buf[padLen - 5] = hi & 0xff;
    buf[padLen - 4] = (lo >>> 24) & 0xff;
    buf[padLen - 3] = (lo >>> 16) & 0xff;
    buf[padLen - 2] = (lo >>> 8) & 0xff;
    buf[padLen - 1] = lo & 0xff;

    const H = new Uint32Array([
      0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
      0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ]);
    const W = new Uint32Array(64);
    const view = new DataView(buf.buffer);
    for (let i = 0; i < padLen; i += 64) {
      for (let t = 0; t < 16; t++) W[t] = view.getUint32(i + t * 4, false);
      for (let t = 16; t < 64; t++) {
        const s0 = rotr(W[t - 15], 7) ^ rotr(W[t - 15], 18) ^ (W[t - 15] >>> 3);
        const s1 = rotr(W[t - 2], 17) ^ rotr(W[t - 2], 19) ^ (W[t - 2] >>> 10);
        W[t] = (W[t - 16] + s0 + W[t - 7] + s1) >>> 0;
      }
      let [a, b, c, d, e, f, g, h] = H;
      for (let t = 0; t < 64; t++) {
        const S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        const ch = (e & f) ^ (~e & g);
        const t1 = (h + S1 + ch + K[t] + W[t]) >>> 0;
        const S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
        const mj = (a & b) ^ (a & c) ^ (b & c);
        const t2 = (S0 + mj) >>> 0;
        h = g; g = f; f = e; e = (d + t1) >>> 0;
        d = c; c = b; b = a; a = (t1 + t2) >>> 0;
      }
      H[0] = (H[0] + a) >>> 0; H[1] = (H[1] + b) >>> 0;
      H[2] = (H[2] + c) >>> 0; H[3] = (H[3] + d) >>> 0;
      H[4] = (H[4] + e) >>> 0; H[5] = (H[5] + f) >>> 0;
      H[6] = (H[6] + g) >>> 0; H[7] = (H[7] + h) >>> 0;
    }
    const hex = "0123456789abcdef";
    let out = "";
    for (let i = 0; i < 8; i++) {
      const w = H[i];
      for (let s = 28; s >= 0; s -= 4) out += hex[(w >>> s) & 0xf];
    }
    return out;
  }

  return {
    get fields() {
      return _fields;
    },
    set fields(v) {
      _fields = v;
    },
    set(name, value) {
      _fields[name] = value;
    },
    del(name) {
      delete _fields[name];
    },
    now() {
      // µs since epoch；Rust 侧把它当数字读，超 2^53 后失精度但够 transform 用。
      return Date.now() * 1000;
    },
    log(level, msg) {
      _logs.push([String(level), String(msg)]);
    },
    sha256,
    parse_json(s) {
      return JSON.parse(s);
    },
    encode_json(v) {
      return JSON.stringify(v);
    },
    // ---- internal hooks, do not call from user source ----
    __reset(jsonStr) {
      _fields = JSON.parse(jsonStr);
      _logs.length = 0;
    },
    __dump() {
      return JSON.stringify(_fields);
    },
    __drainLogs() {
      const out = _logs.slice();
      _logs.length = 0;
      return out;
    },
  };
})();
