const LOBBY = "replay.server.battlelobby";
const LOBBY_DIRS = [
  ["TempWriteReplayP1"],
  ["Heroes of the Storm", "TempWriteReplayP1"],
];

const handles = { temp: null, replays: null };
let lobbyStamp = null;
let timer = null;

// The picker is missing from an insecure page as well as from a browser without the api.
export function capability() {
  if (typeof window.showDirectoryPicker === "function") {
    return { ok: true, text: "" };
  }
  if (!window.isSecureContext) {
    return {
      ok: false,
      reason: "insecure",
      text: `${location.origin} is not a secure context, so the browser hides folder access.`
        + " Reach this page over https, or run it on localhost.",
    };
  }
  return {
    ok: false,
    reason: "browser",
    text: "This browser has no File System Access API. Chrome and Edge do."
      + " Replays can still be loaded by hand below.",
  };
}

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
  if (!capability().ok) return { temp: false, replays: false };
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

// The picker takes a well-known name or a handle, never a path, so a wine prefix
// can only be reached by hand.
export async function pick(key) {
  const can = capability();
  if (!can.ok) throw new Error(can.text);
  const handle = await window.showDirectoryPicker({
    id: `hots-${key}`,
    mode: "read",
    startIn: handles[key] || handles.replays || (key === "replays" ? "documents" : undefined),
  });
  await remember(key, handle);
  return handle.name;
}

const HINTS = {
  windows: {
    temp: "%TEMP%\\Heroes of the Storm",
    replays: "Documents\\Heroes of the Storm\\Accounts\\<id>\\<id>\\Replays\\Multiplayer",
    how: "Paste the path into the name box of the dialog.",
  },
  linux: {
    temp: "~/Games/heroes-of-the-storm/drive_c/users/$USER/Temp/Heroes of the Storm",
    replays: "~/Games/heroes-of-the-storm/drive_c/users/$USER/Documents/Heroes of the Storm/Accounts/<id>/<id>/Replays/Multiplayer",
    how: "That is the Lutris prefix. Press ctrl-l in the dialog to type a path.",
  },
  mac: {
    temp: "the Temp folder of the wine prefix",
    replays: "Documents/Heroes of the Storm/Accounts/<id>/<id>/Replays/Multiplayer",
    how: "Press shift-cmd-g in the dialog to type a path.",
  },
};

export function hints() {
  const name = (navigator.userAgentData?.platform || navigator.platform || "").toLowerCase();
  if (name.includes("win")) return HINTS.windows;
  if (name.includes("mac") || name.includes("darwin")) return HINTS.mac;
  return HINTS.linux;
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

const isReplay = (name) => name.toLowerCase().endsWith(".stormreplay");

async function bytesOf(file) {
  return new Uint8Array(await file.arrayBuffer());
}

// Both sources answer the same shape, so the backfill has one path.
export async function listReplays() {
  if (!handles.replays) return [];
  const out = [];
  for await (const [name, handle] of handles.replays.entries()) {
    if (handle.kind === "file" && isReplay(name)) {
      out.push({ name, read: async () => bytesOf(await handle.getFile()) });
    }
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

export function fromFiles(files) {
  return [...files]
    .filter((file) => isReplay(file.name))
    .map((file) => ({ name: file.name, read: () => bytesOf(file) }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

export async function lobbyFromFile(file) {
  return bytesOf(file);
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
