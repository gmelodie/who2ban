const LOBBY = "replay.server.battlelobby";
const LOBBY_DIRS = [
  ["TempWriteReplayP1"],
  ["Heroes of the Storm", "TempWriteReplayP1"],
];

const handles = { temp: null, replays: null };
let lobbyStamp = null;
let timer = null;

export const supported = "showDirectoryPicker" in window;

function store(mode) {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open("hots-draft", 1);
    open.onupgradeneeded = () => open.result.createObjectStore("handles");
    open.onerror = () => reject(open.error);
    open.onsuccess = () => resolve(open.result.transaction("handles", mode).objectStore("handles"));
  });
}

function ask(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function remember(key, handle) {
  handles[key] = handle;
  await ask((await store("readwrite")).put(handle, key));
}

export async function restore() {
  if (!supported) return { temp: false, replays: false };
  const saved = await store("readonly");
  for (const key of ["temp", "replays"]) {
    const handle = await ask(saved.get(key)).catch(() => null);
    if (handle && (await handle.queryPermission({ mode: "read" })) === "granted") {
      handles[key] = handle;
    }
  }
  return { temp: !!handles.temp, replays: !!handles.replays };
}

// A saved handle needs a click before the browser hands the permission back.
export async function reconnect() {
  const saved = await store("readonly");
  for (const key of ["temp", "replays"]) {
    if (handles[key]) continue;
    const handle = await ask(saved.get(key)).catch(() => null);
    if (handle && (await handle.requestPermission({ mode: "read" })) === "granted") {
      handles[key] = handle;
    }
  }
  return { temp: !!handles.temp, replays: !!handles.replays };
}

export async function pick(key) {
  const handle = await window.showDirectoryPicker({ id: `hots-${key}`, mode: "read" });
  await remember(key, handle);
  return handle.name;
}

async function child(dir, names) {
  let at = dir;
  for (const name of names) at = await at.getDirectoryHandle(name);
  return at;
}

async function readLobby() {
  for (const names of LOBBY_DIRS) {
    try {
      const dir = await child(handles.temp, names);
      const file = await (await dir.getFileHandle(LOBBY)).getFile();
      if (!file.size) return null;
      const stamp = `${file.size}:${file.lastModified}`;
      if (stamp === lobbyStamp) return null;
      const bytes = new Uint8Array(await file.arrayBuffer());
      return { stamp, bytes };
    } catch {
      continue;
    }
  }
  lobbyStamp = null;
  return null;
}

export async function listReplays() {
  if (!handles.replays) return [];
  const out = [];
  for await (const [name, handle] of handles.replays.entries()) {
    if (handle.kind === "file" && name.toLowerCase().endsWith(".stormreplay")) {
      out.push({ name, handle });
    }
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

export async function readReplay(entry) {
  const file = await entry.handle.getFile();
  return new Uint8Array(await file.arrayBuffer());
}

// The client deletes its temp folder on exit, so every poll re-walks the path.
export function watchLobby(onLobby, everyMs = 400) {
  clearInterval(timer);
  timer = setInterval(async () => {
    if (!handles.temp) return;
    const found = await readLobby();
    if (!found) return;
    try {
      await onLobby(found.bytes);
      lobbyStamp = found.stamp;
    } catch {
      lobbyStamp = null;
    }
  }, everyMs);
}

export function connected() {
  return { temp: !!handles.temp, replays: !!handles.replays };
}
