import * as fs from "./fs.js";
import * as wasm from "./wasm.js";

const el = (id) => document.getElementById(id);
const state = {
  draft: null,
  sort: "games",
  minGames: 3,
  busy: false,
  watching: false,
  step: localStorage.getItem("hots.step") || "lobby",
  status: {},
  battletag: localStorage.getItem("hots.battletag") || "",
};

async function api(path, method = "GET", body) {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body ? { "content-type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw new Error((await res.text()) || res.statusText);
  return res.json();
}

function winrate(h) {
  return h.games ? h.wins / h.games : 0;
}

function sortHeroes(heroes) {
  const rows = [...heroes];
  if (state.sort === "winrate") {
    rows.sort((a, b) => {
      const ha = a.games >= state.minGames;
      const hb = b.games >= state.minGames;
      if (ha !== hb) return ha ? -1 : 1;
      return winrate(b) - winrate(a) || b.games - a.games;
    });
  } else {
    rows.sort((a, b) => b.games - a.games);
  }
  return rows;
}

function bucket(h) {
  if (h.games < state.minGames) return "thin";
  const r = winrate(h);
  if (r >= 0.6) return "hot";
  if (r <= 0.4) return "cold";
  return "mid";
}

function heroRow(h) {
  const rate = h.games >= state.minGames ? `${Math.round(winrate(h) * 100)}%` : "-";
  const tr = document.createElement("tr");
  tr.className = `wr-${bucket(h)}`;
  tr.innerHTML = `<td class="hero">${escape(h.hero)}</td>
    <td class="num">${h.games}</td>
    <td class="num">${h.wins}</td>
    <td class="num">${rate}</td>`;
  return tr;
}

function playerCard(p) {
  const card = document.createElement("article");
  card.className = "card";

  const head = document.createElement("header");
  head.innerHTML = `<span class="tag">${escape(p.battletag)}</span>
    <span class="games">${p.games} games</span>`;
  card.append(head);

  if (!p.heroes.length) {
    const none = document.createElement("p");
    none.className = "none";
    none.textContent = "no games on record";
    card.append(none);
    return card;
  }

  const table = document.createElement("table");
  table.innerHTML = "<thead><tr><th>hero</th><th>games</th><th>won</th><th>win</th></tr></thead>";
  const body = document.createElement("tbody");
  sortHeroes(p.heroes).forEach((h) => body.append(heroRow(h)));
  table.append(body);
  card.append(table);
  return card;
}

function render() {
  const box = el("players");
  box.textContent = "";
  if (!state.draft) {
    box.innerHTML = '<p class="empty">Waiting for a lobby. Start a game.</p>';
    return;
  }
  const shown = state.draft.my_team === null
    ? state.draft.players
    : state.draft.players.filter((p) => p.enemy);
  if (state.draft.my_team === null) {
    showError("Your battletag is not in this lobby, so every player is shown.");
  }
  shown.sort((a, b) => a.slot - b.slot).forEach((p) => box.append(playerCard(p)));
}

function ago(at) {
  const seconds = Math.round((Date.now() - at) / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  return `${Math.round(seconds / 60)} min ago`;
}

// Says whether the folder was readable a moment ago, not merely picked once.
function watchHealth() {
  const s = state.status;
  if (state.watching) {
    return s.temp_root_exists
      ? "the folder is there"
      : "the folder is not there yet, which is normal while the game is closed";
  }
  const health = fs.healthOf("temp");
  if (!fs.connected().temp) return "";
  if (!health) return "not checked yet";
  if (health.permission !== "granted") return `permission ${health.permission}, click connect again`;
  return health.readable
    ? `permission granted, read ${ago(health.at)}`
    : `permission granted but the folder would not open, checked ${ago(health.at)}`;
}

function showError(text) {
  const line = document.createElement("div");
  line.textContent = text;
  el("errors").prepend(line);
  setTimeout(() => line.remove(), 15000);
}

function escape(text) {
  const node = document.createElement("span");
  node.textContent = text;
  return node.innerHTML;
}

function note(text) {
  el("progress").textContent = text;
}

function showStep() {
  const lobby = state.step === "lobby";
  el("step-replays").hidden = lobby;
  el("step-lobby").hidden = !lobby;
  el("go-replays").classList.toggle("here", !lobby);
  el("go-lobby").classList.toggle("here", lobby);
  localStorage.setItem("hots.step", state.step);
  if (lobby) showLobbyStep();
}

function goto(step) {
  state.step = step;
  showStep();
}

async function onLobby(bytes) {
  state.draft = await api("/draft", "POST", {
    lobby: wasm.parseLobby(bytes),
    battletag: state.battletag || null,
  });
  goto("lobby");
  render();
}

// A file input hands back everything the folder held, so say when none of it was a replay.
function chose(files) {
  const entries = fs.fromFiles(files);
  if (!entries.length) {
    showError(`none of the ${files.length} files end in .StormReplay`);
    return;
  }
  return backfill(entries);
}

async function backfill(entries) {
  const list = entries || (fs.connected().replays ? await fs.listReplays() : []);
  if (state.busy || !list.length) return;
  state.busy = true;
  try {
    const known = new Set(await api("/matches/known"));
    const files = list.filter((f) => !known.has(f.name));
    note(files.length ? `parsing ${files.length} replays` : "nothing new to parse");
    let failed = 0;
    for (const [i, entry] of files.entries()) {
      note(`parsing replays ${i + 1}/${files.length}`);
      try {
        const record = wasm.parseReplay(await entry.read());
        await api("/matches", "POST", { key: entry.name, record });
      } catch (e) {
        failed += 1;
        if (failed < 4) showError(`${entry.name}: ${e.message}`);
      }
      await new Promise((resume) => setTimeout(resume, 0));
    }
    note(failed ? `${failed} of ${files.length} could not be read` : "");
    await loadStatus();
  } finally {
    state.busy = false;
  }
}

async function loadStatus() {
  const s = await api("/status");
  state.status = s;
  state.watching = s.watching;
  el("status").textContent = `${s.matches} replays · ${s.failed} unreadable`;
  el("replays-count").textContent = `${s.matches} replays stored`;
  showFolders();
  showLobbyStep();
  return s;
}

// A disabled button with no reason beside it is the same as a broken one.
function showFolders() {
  const at = fs.connected();
  const can = fs.capability();
  const hints = fs.hints();

  el("pick-replays").disabled = !can.ok;
  el("replays-why").textContent = at.replays
    ? `reading ${at.replays}`
    : can.ok
      ? ""
      : "this browser has no folder access, so use the file picker below";
  el("replays-hint").textContent = at.replays || !can.ok ? "" : hints.replays;
  el("copy-replays").hidden = !!at.replays || !can.ok;
  el("how").textContent = at.replays || !can.ok ? "" : hints.how;
}

function showLobbyStep() {
  const at = fs.connected();
  const can = fs.capability();
  const hints = fs.hints();
  const blocked = !!hints.tempBlocked;
  const s = state.status;

  const [label, path] = state.watching
    ? ["the server is watching", s.temp_root]
    : at.temp
      ? ["this browser is watching", at.temp]
      : blocked
        ? ["the browser cannot open the temp folder", ""]
        : ["nothing is watching for a lobby", ""];
  el("lobby-state").textContent = label;
  el("lobby-path").textContent = path;
  el("lobby-health").textContent = watchHealth();

  const hide = state.watching || !!at.temp;
  el("pick-temp").hidden = hide;
  el("pick-temp").disabled = !can.ok || blocked;
  el("temp-hint").textContent = hide || blocked ? "" : hints.temp;
  el("copy-temp").hidden = hide || blocked || !hints.temp;
  el("manual-lobby-box").hidden = state.watching || (can.ok && !blocked);

  if (s.watch_error) {
    el("banner").textContent = `The server could not watch its folders: ${s.watch_error}`;
    el("banner").hidden = false;
  } else if (!state.watching && blocked && !at.temp) {
    el("banner").textContent = hints.tempBlocked;
    el("banner").hidden = false;
  } else if (state.watching) {
    el("banner").hidden = true;
  }
}

async function loadConfig() {
  const cfg = await api("/config");
  state.minGames = cfg.min_games_for_winrate;
  el("maxheroes").value = cfg.max_heroes;
  el("mingames").value = cfg.min_games_for_winrate;
  el("allmodes").checked = cfg.local_all_modes;
}

async function save() {
  const cfg = await api("/config");
  cfg.max_heroes = Number(el("maxheroes").value);
  cfg.min_games_for_winrate = Number(el("mingames").value);
  cfg.local_all_modes = el("allmodes").checked;
  try {
    await api("/config", "PUT", cfg);
    state.minGames = cfg.min_games_for_winrate;
    render();
  } catch (e) {
    showError(e.message);
  }
}

function subscribe() {
  const events = new EventSource("/api/events");
  events.addEventListener("lobby", (e) => {
    state.draft = JSON.parse(e.data);
    goto("lobby");
    render();
  });
  events.addEventListener("ingested", loadStatus);
  events.addEventListener("lobby-error", (e) => showError(`lobby: ${JSON.parse(e.data)}`));
}

async function copy(button, text) {
  try {
    await navigator.clipboard.writeText(text);
    button.textContent = "copied";
  } catch {
    button.textContent = "select it and copy";
  }
  setTimeout(() => (button.textContent = "copy"), 2000);
}

async function pick(key) {
  try {
    await fs.pick(key);
    showFolders();
    showLobbyStep();
    if (key === "replays") backfill();
    await fs.probe(key);
    showLobbyStep();
    if (key === "temp") fs.watchLobby(onLobby);
  } catch (e) {
    if (e.name !== "AbortError") showError(e.message);
  }
}

async function main() {
  el("go-replays").onclick = () => goto("replays");
  el("go-lobby").onclick = () => goto("lobby");
  el("to-lobby").onclick = () => goto("lobby");
  el("settings-toggle").onclick = () => {
    el("settings").hidden = !el("settings").hidden;
  };
  el("save").onclick = save;
  el("sort").onchange = (e) => {
    state.sort = e.target.value;
    render();
  };
  el("battletag").value = state.battletag;
  el("battletag").onchange = (e) => {
    state.battletag = e.target.value.trim();
    localStorage.setItem("hots.battletag", state.battletag);
  };
  el("pick-replays").onclick = () => pick("replays");
  el("pick-temp").onclick = () => pick("temp");
  el("copy-replays").onclick = () => copy(el("copy-replays"), fs.hints().replays);
  el("copy-temp").onclick = () => copy(el("copy-temp"), fs.hints().temp);
  el("manual-replays").onchange = (e) => chose(e.target.files);
  el("manual-replays-dir").onchange = (e) => chose(e.target.files);
  el("manual-lobby").onchange = async (e) => {
    const [file] = e.target.files;
    if (!file) return;
    try {
      await onLobby(await fs.lobbyFromFile(file));
    } catch (err) {
      showError(err.message);
    }
  };

  setInterval(() => {
    if (state.step === "lobby") el("lobby-health").textContent = watchHealth();
  }, 2000);

  await wasm.load().catch((e) => showError(`parser: ${e.message}`));
  await loadConfig();
  const status = await loadStatus();
  await fs.restore();
  await Promise.all([fs.probe("temp"), fs.probe("replays")]);

  if (!status.matches && !localStorage.getItem("hots.step")) state.step = "replays";
  showStep();
  showFolders();
  subscribe();

  state.draft = await api("/draft");
  render();

  if (!state.watching && fs.capability().ok) {
    fs.watchLobby(onLobby);
    setInterval(() => backfill(), 60_000);
    backfill();
  }
}

main();
