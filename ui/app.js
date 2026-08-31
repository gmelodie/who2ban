const el = (id) => document.getElementById(id);
const state = { minGames: 3 };

async function api(path, method = "GET", body) {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body ? { "content-type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw new Error((await res.text()) || res.statusText);
  return res.json();
}

function escape(text) {
  const node = document.createElement("span");
  node.textContent = text;
  return node.innerHTML;
}

function showError(text) {
  const line = document.createElement("div");
  line.textContent = text;
  el("errors").prepend(line);
  setTimeout(() => line.remove(), 15000);
}

function winrate(h) {
  return h.games ? h.wins / h.games : 0;
}

function heroTable(rows) {
  const table = document.createElement("table");
  table.innerHTML = "<thead><tr><th>hero</th><th>games</th><th>won</th><th>win</th></tr></thead>";
  const body = document.createElement("tbody");
  for (const h of rows) {
    const rate = h.games >= state.minGames ? `${Math.round(winrate(h) * 100)}%` : "-";
    const tr = document.createElement("tr");
    tr.innerHTML = `<td class="hero">${escape(h.hero)}</td>
      <td class="num">${h.games}</td><td class="num">${h.wins}</td><td class="num">${rate}</td>`;
    body.append(tr);
  }
  table.append(body);
  return table;
}

async function find() {
  const battletag = el("lookup").value.trim();
  const box = el("player");
  box.textContent = "";
  if (!battletag) return;
  try {
    const row = await api(`/player/${encodeURIComponent(battletag)}`);
    const card = document.createElement("article");
    card.className = "card";
    card.innerHTML = `<header><span class="tag">${escape(row.battletag)}</span>
      <span class="games">${row.games} games</span></header>`;
    if (row.heroes.length) card.append(heroTable(row.heroes));
    else card.insertAdjacentHTML("beforeend", '<p class="none">no games on record</p>');
    box.append(card);
  } catch (e) {
    showError(e.message);
  }
}

function when(seconds) {
  return new Date(seconds * 1000).toISOString().slice(0, 16).replace("T", " ");
}

async function loadRecent() {
  const rows = await api("/matches/recent");
  const body = el("recent").querySelector("tbody");
  body.textContent = "";
  for (const m of rows) {
    const tr = document.createElement("tr");
    tr.innerHTML = `<td>${when(m.played_at)}</td><td>${escape(m.map)}</td>
      <td>${escape(m.mode)}</td><td class="num">${m.players}</td><td class="num">${m.files}</td>`;
    body.append(tr);
  }
  if (!rows.length) body.innerHTML = '<tr><td colspan="5" class="none">nothing stored yet</td></tr>';
}

async function loadStatus() {
  const s = await api("/status");
  el("status").textContent =
    `${s.matches} matches from ${s.files} replay files · ${s.failed} unreadable`;
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
    await find();
  } catch (e) {
    showError(e.message);
  }
}

async function main() {
  el("save").onclick = save;
  el("find").onclick = find;
  el("lookup").onkeydown = (e) => {
    if (e.key === "Enter") find();
  };
  await loadConfig();
  await loadStatus();
  await loadRecent();
  setInterval(() => loadStatus().then(loadRecent).catch(() => {}), 15000);
}

main();
