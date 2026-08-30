const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const el = (id) => document.getElementById(id);
const state = { draft: null, sort: "games", minGames: 3 };

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

function bucket(h) {
  if (h.games < state.minGames) return "thin";
  const r = winrate(h);
  if (r >= 0.6) return "hot";
  if (r <= 0.4) return "cold";
  return "mid";
}

function stateLabel(p) {
  if (p.error) return escape(`failed: ${p.error}`);
  return { fresh: "HP", stale: "HP stale", pending: "fetching…", missing: "local only", failed: "failed" }[p.hp_state];
}

function playerCard(p) {
  const card = document.createElement("article");
  card.className = `card ${p.hp_state}`;
  card.dataset.battletag = p.battletag;

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
      replacePlayer(await invoke("refresh_player", { battletag: p.battletag, region: p.region }));
    } catch (e) {
      showError(`${p.battletag}: ${e}`);
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

async function loadStatus() {
  const s = await invoke("status");
  el("status").textContent =
    `${s.matches} replays · ${s.failed} unreadable · ${s.battletag || "battletag unset"}` +
    (s.has_api_key ? "" : " · no api key");
  el("paths").textContent = [`temp: ${s.temp_root}`, ...s.replay_dirs.map((d) => `replays: ${d}`)].join("\n");
}

async function loadConfig() {
  const cfg = await invoke("get_config");
  state.minGames = cfg.min_games_for_winrate;
  el("battletag").value = cfg.battletag || "";
  el("apikey").value = cfg.hp_api_key || "";
  el("gametype").value = cfg.hp_game_type;
  el("ttl").value = cfg.hp_ttl_days;
  el("maxheroes").value = cfg.max_heroes;
  el("allmodes").checked = cfg.local_all_modes;
  return cfg;
}

async function save() {
  const cfg = await invoke("get_config");
  cfg.battletag = el("battletag").value.trim() || null;
  cfg.hp_api_key = el("apikey").value.trim() || null;
  cfg.hp_game_type = el("gametype").value.trim();
  cfg.hp_ttl_days = Number(el("ttl").value);
  cfg.max_heroes = Number(el("maxheroes").value);
  cfg.local_all_modes = el("allmodes").checked;
  try {
    await invoke("set_config", { cfg });
    state.minGames = cfg.min_games_for_winrate;
    await loadStatus();
    render();
  } catch (e) {
    showError(String(e));
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

  await loadConfig();
  await loadStatus();
  state.draft = await invoke("current_draft");
  render();

  listen("lobby", (e) => {
    state.draft = e.payload;
    render();
  });
  listen("player", (e) => replacePlayer(e.payload));
  listen("ingest", (e) => {
    const p = e.payload;
    if (p.total) el("status").textContent = `parsing replays ${p.done}/${p.total}`;
    if (p.done === p.total) loadStatus();
  });
  listen("ingested", loadStatus);
  listen("lobby-error", (e) => showError(`lobby: ${e.payload}`));
  listen("hp-error", (e) => showError(`heroes profile: ${e.payload}`));
}

main();
