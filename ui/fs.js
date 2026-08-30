const LOBBY = "replay.server.battlelobby";
const LOBBY_DIRS = [
  ["TempWriteReplayP1"],
  ["Heroes of the Storm", "TempWriteReplayP1"],
];

const handles = { temp: null, replays: null };
const health = { temp: null, replays: null };
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
  const handle = await window
    .showDirectoryPicker({
      id: `hots-${key}`,
      mode: "read",
      startIn: handles[key] || handles.replays || (key === "replays" ? "documents" : undefined),
    })
    .catch(refuse);
  await remember(key, handle);
  return handle.name;
}

// Each path is one a folder dialog reaches and one that exists with the game closed.
// The browser blocks a whole list of folders, AppData among them, and says so vaguely.
function refuse(e) {
  if (e.name === "AbortError") throw e;
  const blocked = e.name === "SecurityError" || /system files|not allowed/i.test(e.message);
  throw blocked ? new Error(hints().tempBlocked || e.message) : e;
}

const HINTS = {
  windows: {
    temp: "",
    tempBlocked:
      "Windows keeps the temp folder inside AppData, and the browser refuses every folder in there:"
      + " it answers that the folder contains system files. Run the server on this machine with"
      + " `make serve` and it reads the folder directly, or load a battlelobby by hand below.",
    replays: "%USERPROFILE%\\Documents\\Heroes of the Storm",
    how: "Copy the path, paste it into the File name box of the dialog, press enter, then Select Folder.",
  },
  linux: {
    temp: "~/Games/battlenet/drive_c/users/steamuser/AppData/Local/Temp",
    tempBlocked: "",
    replays: "~/Games/battlenet/drive_c/users/steamuser/Documents/Heroes of the Storm",
    how: "Copy a path, press ctrl-l in the dialog, paste it, press enter. Your prefix may sit elsewhere under ~/Games.",
  },
  mac: {
    temp: "~/Library/Caches/TemporaryItems",
    tempBlocked: "",
    replays: "~/Documents/Heroes of the Storm",
    how: "Copy a path, press shift-cmd-g in the dialog, paste it, press enter.",
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

// Any folder above the replays will do, so nobody has to find Replays/Multiplayer.
export async function listReplays() {
  if (!handles.replays) return [];
  const out = [];
  await collect(handles.replays, out, 6);
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

async function collect(dir, out, depth) {
  if (depth === 0) return;
  for await (const [name, handle] of dir.entries()) {
    if (handle.kind === "file" && isReplay(name)) {
      out.push({ name, read: async () => bytesOf(await handle.getFile()) });
    } else if (handle.kind === "directory") {
      await collect(handle, out, depth - 1);
    }
  }
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
  let ticks = 0;
  timer = setInterval(async () => {
    if (!handles.temp) return;
    if (ticks++ % 10 === 0) await probe("temp");
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

// The api gives a folder its name and never its path, so the name is all there is to show.
export function connected() {
  return { temp: handles.temp?.name || null, replays: handles.replays?.name || null };
}

// A granted permission is not a readable folder, so prove it by reading one entry.
export async function probe(key) {
  const handle = handles[key];
  if (!handle) {
    health[key] = null;
    return null;
  }
  const permission = await handle.queryPermission({ mode: "read" }).catch(() => "denied");
  let readable = false;
  if (permission === "granted") {
    readable = await handle
      .entries()
      .next()
      .then(() => true, () => false);
  }
  health[key] = { permission, readable, at: Date.now() };
  return health[key];
}

export function healthOf(key) {
  return health[key];
}
