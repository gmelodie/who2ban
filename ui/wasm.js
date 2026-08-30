let api = null;

export async function load() {
  if (api) return api;
  const { instance } = await WebAssembly.instantiateStreaming(fetch("/hots_parse.wasm"), {});
  api = instance.exports;
  return api;
}

function call(fn, bytes) {
  const input = api.hots_alloc(bytes.length);
  new Uint8Array(api.memory.buffer, input, bytes.length).set(bytes);

  const out = fn(input, bytes.length);
  const len = new DataView(api.memory.buffer).getUint32(out, true);
  const json = new TextDecoder().decode(new Uint8Array(api.memory.buffer, out + 4, len));

  api.hots_free(out, 4 + len);
  api.hots_free(input, bytes.length);

  const value = JSON.parse(json);
  if (value.error) throw new Error(value.error);
  return value;
}

export function parseLobby(bytes) {
  return call(api.hots_parse_lobby, bytes);
}

export function parseReplay(bytes) {
  return call(api.hots_parse_replay, bytes);
}
