import * as fs from "./fs.js";
import * as wasm from "./wasm.js";

const el = (id) => document.getElementById(id);
const state = { draft: null, sort: "games", minGames: 3, busy: false };

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
    <td class="num">${rate}</td>
    <td class="src ${h.source}">${h.source}</td>`;
  return tr;
}

function stateLabel(p) {
  if (p.error) return escape(`failed: ${p.error}`);
  return { fresh: "HP", stale: "HP stale", pending: "fetching…", missing: "local only", failed: "failed" }[p.hp_state];
}

function playerCard(p) {
  const card = document.createElement("article");
  card.className = `card ${p.hp_state}`;

  const mmr = p.mmr ? `${Math.round(p.mmr)} mmr` : "";
  const head = document.createElement("header");
  head.innerHTML = `<span class="tag">${escape(p.battletag)}</span>
    <span class="mmr">${mmr}</span>
    <span class="badge ${p.hp_state}">${stateLabel(p)}</span>`;
  head.append(refreshButton(p));
  card.append(head);

  if (!p.heroes.length) {
    const none = document.createElement("p");
    none.className = "none";
    none.textContent = p.hp_state === "pending" ? "…" : "no games on record";
    card.append(none);
    return card;
  }

  const table = document.createElement("table");
  table.innerHTML = "<thead><tr><th>hero</th><th>games</th><th>win</th><th>src</th></tr></thead>";
  const body = document.createElement("tbody");
  sortHeroes(p.heroes).forEach((h) => body.append(heroRow(h)));
  table.append(body);
  card.append(table);
  return card;
}

function refreshButton(p) {
  const button = document.createElement("button");
  button.className = "refresh";
  button.textContent = "↻";
  button.onclick = async () => {
    button.disabled = true;
    try {
      const body = { battletag: p.battletag, region: p.region };
      replacePlayer(await api("/player/refresh", "POST", body));
    } catch (e) {
      showError(`${p.battletag}: ${e.message}`);
    } finally {
      button.disabled = false;
    }
  };
  return button;
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

function replacePlayer(row) {
  if (!state.draft) return;
  const i = state.draft.players.findIndex((p) => p.battletag === row.battletag);
  if (i < 0) return;
  state.draft.players[i] = { ...state.draft.players[i], ...row };
  render();
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

async function backfill() {
  if (state.busy || !fs.connected().replays) return;
  state.busy = true;
  try {
    const known = new Set(await api("/matches/known"));
    const files = (await fs.listReplays()).filter((f) => !known.has(f.name));
    for (const [i, entry] of files.entries()) {
      note(`parsing replays ${i + 1}/${files.length}`);
      try {
        const record = wasm.parseReplay(await fs.readReplay(entry));
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
  el("status").textContent =
    `${s.matches} replays · ${s.failed} unreadable · ${s.battletag || "battletag unset"}` +
    (s.has_api_key ? "" : " · no api key");
}

function showFolders() {
  const at = fs.connected();
  el("temp-state").textContent = at.temp ? "connected" : "not connected";
  el("replays-state").textContent = at.replays ? "connected" : "not connected";
  el("reconnect").hidden = at.temp && at.replays;
}

async function loadConfig() {
  const cfg = await api("/config");
  state.minGames = cfg.min_games_for_winrate;
  el("battletag").value = cfg.battletag || "";
  el("apikey").value = cfg.hp_api_key || "";
  el("gametype").value = cfg.hp_game_type;
  el("ttl").value = cfg.hp_ttl_days;
  el("maxheroes").value = cfg.max_heroes;
  el("allmodes").checked = cfg.local_all_modes;
}

async function save() {
  const cfg = await api("/config");
  cfg.battletag = el("battletag").value.trim() || null;
  cfg.hp_api_key = el("apikey").value.trim() || null;
  cfg.hp_game_type = el("gametype").value.trim();
  cfg.hp_ttl_days = Number(el("ttl").value);
  cfg.max_heroes = Number(el("maxheroes").value);
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
  events.addEventListener("player", (e) => replacePlayer(JSON.parse(e.data)));
  events.addEventListener("ingested", loadStatus);
  events.addEventListener("hp-error", (e) => showError(`heroes profile: ${JSON.parse(e.data)}`));
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
  el("reconnect").onclick = async () => {
    await fs.reconnect();
    showFolders();
    backfill();
  };

  if (!fs.supported) {
    note("This browser has no File System Access API. Use Chrome or Edge.");
  }

  await wasm.load();
  await loadConfig();
  await loadStatus();
  await fs.restore();
  showFolders();
  subscribe();

  state.draft = await api("/draft");
  render();

  fs.watchLobby(onLobby);
  backfill();
  setInterval(backfill, 60_000);
}

main();
