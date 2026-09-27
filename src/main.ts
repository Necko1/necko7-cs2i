import { invoke } from "@tauri-apps/api/core";
import { element, render, type Status } from "./ui";
import "./styles.css";

element("app").innerHTML = `
<header><h1 id="connection">Connecting…</h1><button id="settings-button" class="secondary" aria-label="Settings" aria-haspopup="dialog">⚙</button></header>
<section id="pairing"><form id="pair-form"><label for="code">Pairing code</label><input id="code" maxlength="9" placeholder="7K3M-PQ8D" autocomplete="off" spellcheck="false" required><button id="connect" type="submit">Connect</button></form><p class="muted">Get a pairing code in your channel dashboard.</p><label class="toggle"><input id="onboarding-autostart" type="checkbox">Start with Windows</label></section>
<section id="channel" hidden><img id="avatar" alt="Twitch avatar" referrerpolicy="no-referrer"><h2 id="display-name"></h2><p id="username"></p><p id="twitch-id" class="muted"></p></section>
<p id="error" role="alert"></p><p id="identity" role="alert"></p>
<div class="statuses"><div class="status-row"><strong>CS2</strong><div><p id="gsi"></p><p id="listener" class="muted"></p></div></div><button id="retry" class="secondary">Retry discovery</button><div class="status-row"><strong>Cloud</strong><p id="cloud"></p></div></div>
<dialog id="settings" aria-labelledby="settings-title"><div class="dialog-header"><h2 id="settings-title">Settings</h2><button id="settings-close" class="secondary" aria-label="Close settings">✕</button></div><label class="toggle"><input id="autostart" type="checkbox">Start with Windows</label><label class="toggle"><input id="minimize" type="checkbox">Minimize to tray when closing</label><p id="settings-error" role="alert"></p><button id="unpair" class="secondary">Unpair desktop</button><button id="reset" class="secondary" hidden>Reset unavailable identity</button><p id="version" class="muted"></p></dialog>`;
const settings = element<HTMLDialogElement>("settings");
function showError(message: string) { element(settings.open ? "settings-error" : "error").textContent = message; }
let busy = false;
let localError = "";
async function refresh() {
  if (busy) return;
  try { const status = await invoke<Status>("status"); render(status); if (status.pairing_busy && settings.open) settings.close(); showError(status.error ?? localError); }
  catch { showError("Desktop services unavailable. Restart the app."); }
}
async function action(command: string, args?: Record<string, unknown>) {
  if (busy) return;
  busy = true; localError = ""; element("error").textContent = ""; element("settings-error").textContent = "";
  element<HTMLButtonElement>("connect").disabled = true;
  if (command === "pair") element("connect").textContent = "Connecting…";
  try { await invoke(command, args); } catch (error) { localError = String(error); showError(localError); }
  finally { busy = false; await refresh(); }
}
element("pair-form").addEventListener("submit", event => { event.preventDefault(); void action("pair", { code: element<HTMLInputElement>("code").value }); });
element("settings-button").addEventListener("click", () => settings.showModal());
element("settings-close").addEventListener("click", () => settings.close());
settings.addEventListener("click", event => {
  if (event.target === settings) {
    const r = settings.getBoundingClientRect();
    if (event.clientX < r.left || event.clientX > r.right || event.clientY < r.top || event.clientY > r.bottom) settings.close();
  }
});
settings.addEventListener("close", () => element("settings-button").focus());
element("retry").addEventListener("click", () => void action("retry_discovery"));
element("unpair").addEventListener("click", () => void action("unpair"));
element("reset").addEventListener("click", () => {
  if (window.confirm("First revoke the old device in the web dashboard. Reset the unavailable local identity now?")) void action("reset_identity");
});
for (const id of ["autostart", "minimize"]) element(id).addEventListener("change", () => void action("preferences", { autostart: element<HTMLInputElement>("autostart").checked, minimize: element<HTMLInputElement>("minimize").checked }));
element("onboarding-autostart").addEventListener("change", () => void action("preferences", { autostart: element<HTMLInputElement>("onboarding-autostart").checked, minimize: element<HTMLInputElement>("minimize").checked }));
void refresh(); setInterval(() => void refresh(), 2000);
