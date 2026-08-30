import * as fs from "./fs.js";
import * as wasm from "./wasm.js";

const el = (id) => document.getElementById(id);
const state = { draft: null, sort: "games", minGames: 3, busy: false, watching: false };

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
  shown.sort((a, b) => a.slot - b.slot).forEach((p) => box.append(playerCard(p)));
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

async function onLobby(bytes) {
  state.draft = await api("/draft", "POST", wasm.parseLobby(bytes));
  render();
}

async function backfill(entries) {
  const list = entries || (fs.connected().replays ? await fs.listReplays() : []);
  if (state.busy || !list.length) return;
  state.busy = true;
  try {
    const known = new Set(await api("/matches/known"));
    const files = list.filter((f) => !known.has(f.name));
    for (const [i, entry] of files.entries()) {
      note(`parsing replays ${i + 1}/${files.length}`);
      try {
        const record = wasm.parseReplay(await entry.read());
        await api("/matches", "POST", { key: entry.name, record });
      } catch (e) {
        showError(`${entry.name}: ${e.message}`);
      }
      await new Promise((resume) => setTimeout(resume, 0));
    }
    note("");
    await loadStatus();
  } finally {
    state.busy = false;
  }
}

async function loadStatus() {
  const s = await api("/status");
  state.watching = s.watching;
  el("status").textContent =
    `${s.matches} replays · ${s.failed} unreadable · ${s.battletag || "battletag unset"}`;
}

// The server reading the folders makes every browser equal, so the page asks for nothing.
function showFolders() {
  el("folders").hidden = state.watching;
  el("banner").hidden = true;
  if (state.watching) return;

  const at = fs.connected();
  const can = fs.capability();
  const hints = fs.hints();
  const blocked = !!hints.tempBlocked;

  if (!can.ok) {
    el("banner").textContent = `${can.text} Or run the server on this machine, where it reads the folders itself.`;
    el("banner").hidden = false;
  } else if (blocked && !at.temp) {
    el("banner").textContent = hints.tempBlocked;
    el("banner").hidden = false;
  }

  el("pick-temp").disabled = !can.ok || blocked;
  el("pick-replays").disabled = !can.ok;
  el("manual-replays-box").hidden = can.ok;
  el("manual-lobby-box").hidden = can.ok && !blocked;
  el("temp-state").textContent = at.temp ? "connected" : blocked ? "blocked by the browser" : "not connected";
  el("replays-state").textContent = at.replays ? "connected" : "not connected";
  el("temp-hint").textContent = at.temp ? "" : hints.temp;
  el("replays-hint").textContent = at.replays ? "" : hints.replays;
  el("copy-temp").hidden = at.temp || !hints.temp;
  el("copy-replays").hidden = at.replays;
  el("how").textContent = at.temp && at.replays ? "" : hints.how;
  el("reconnect").hidden = !can.ok || (at.temp && at.replays);
}

async function loadConfig() {
  const cfg = await api("/config");
  state.minGames = cfg.min_games_for_winrate;
  el("battletag").value = cfg.battletag || "";
  el("maxheroes").value = cfg.max_heroes;
  el("mingames").value = cfg.min_games_for_winrate;
  el("allmodes").checked = cfg.local_all_modes;
}

async function save() {
  const cfg = await api("/config");
  cfg.battletag = el("battletag").value.trim() || null;
  cfg.max_heroes = Number(el("maxheroes").value);
  cfg.min_games_for_winrate = Number(el("mingames").value);
  cfg.local_all_modes = el("allmodes").checked;
  try {
    await api("/config", "PUT", cfg);
    state.minGames = cfg.min_games_for_winrate;
    await loadStatus();
    render();
  } catch (e) {
    showError(e.message);
  }
}

function subscribe() {
  const events = new EventSource("/api/events");
  events.addEventListener("lobby", (e) => {
    state.draft = JSON.parse(e.data);
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
    if (key === "replays") backfill();
  } catch (e) {
    if (e.name !== "AbortError") showError(e.message);
  }
}

async function main() {
  el("sort").onchange = (e) => {
    state.sort = e.target.value;
    render();
  };
  el("settings-toggle").onclick = () => {
    el("settings").hidden = !el("settings").hidden;
  };
  el("save").onclick = save;
  el("pick-temp").onclick = () => pick("temp");
  el("pick-replays").onclick = () => pick("replays");
  el("copy-temp").onclick = () => copy(el("copy-temp"), fs.hints().temp);
  el("copy-replays").onclick = () => copy(el("copy-replays"), fs.hints().replays);
  el("manual-replays").onchange = (e) => backfill(fs.fromFiles(e.target.files));
  el("manual-lobby").onchange = async (e) => {
    const [file] = e.target.files;
    if (!file) return;
    try {
      await onLobby(await fs.lobbyFromFile(file));
    } catch (err) {
      showError(err.message);
    }
  };
  el("reconnect").onclick = async () => {
    await fs.reconnect();
    showFolders();
    backfill();
  };

  // A missing module must not take the settings panel down with it.
  await wasm.load().catch((e) => showError(`parser: ${e.message}`));
  await loadConfig();
  await loadStatus();
  await fs.restore();
  showFolders();
  subscribe();

  state.draft = await api("/draft");
  render();

  if (!state.watching && fs.capability().ok) {
    fs.watchLobby(onLobby);
    setInterval(() => backfill(), 60_000);
  }
  if (!state.watching) backfill();
}

main();
