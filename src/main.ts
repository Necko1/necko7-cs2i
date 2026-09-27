import { invoke } from "@tauri-apps/api/core";
import { element, render, type Status } from "./ui";
import "./styles.css";

element("app").innerHTML = `
  <header><span class="brand">necko7</span><span>CS2 Integration</span><button id="settings-button" aria-label="Settings" aria-expanded="false">⚙</button></header>
  <h1 id="connection">Connecting…</h1>
  <section id="pairing"><form id="pair-form"><label for="code">Pairing code</label><input id="code" maxlength="9" placeholder="7K3M-PQ8D" autocomplete="off" spellcheck="false" required><button id="connect" type="submit">Connect</button></form><p class="muted">Open CS2 Integration in your channel dashboard to get a pairing code.</p><label class="toggle"><input id="onboarding-autostart" type="checkbox">Start with Windows</label></section>
  <section id="channel" hidden><img id="avatar" alt="Twitch avatar" referrerpolicy="no-referrer"><h2 id="display-name"></h2><p id="username"></p><p id="twitch-id" class="muted"></p></section>
  <section><h2>CS2</h2><p id="gsi"></p><p id="listener" class="muted"></p><button id="retry" class="secondary">Retry discovery</button></section>
  <section><h2>Cloud</h2><p id="cloud"></p></section>
  <p id="error" role="alert"></p><p id="identity" role="alert"></p>
  <section id="settings" hidden><h2>Settings</h2><label class="toggle"><input id="autostart" type="checkbox">Start with Windows</label><label class="toggle"><input id="minimize" type="checkbox">Minimize to tray when closing</label><button id="unpair" class="secondary">Unpair desktop</button><button id="reset" class="secondary" hidden>Reset unavailable identity</button><p id="version" class="muted"></p></section>
  <footer><strong>Coming soon</strong><p>CS2 event rewards</p><p class="muted">Startup is optional. Enable “Start with Windows” in Settings.</p></footer>`;
let busy = false;
async function refresh() {
  if (busy) return;
  try { const status = await invoke<Status>("status"); render(status); if (status.error) element("error").textContent = status.error; }
  catch { element("error").textContent = "Desktop services unavailable. Restart the app."; }
}
async function action(command: string, args?: Record<string, unknown>) {
  if (busy) return;
  busy = true; element("error").textContent = "";
  element<HTMLButtonElement>("connect").disabled = true;
  try { await invoke(command, args); } catch (error) { element("error").textContent = String(error); }
  finally { busy = false; await refresh(); }
}
element("pair-form").addEventListener("submit", event => { event.preventDefault(); void action("pair", { code: element<HTMLInputElement>("code").value }); });
element("settings-button").addEventListener("click", () => { const panel = element("settings"); panel.hidden = !panel.hidden; element("settings-button").setAttribute("aria-expanded",String(!panel.hidden)); });
element("retry").addEventListener("click", () => void action("retry_discovery"));
element("unpair").addEventListener("click", () => void action("unpair"));
element("reset").addEventListener("click", () => {
  if (window.confirm("First revoke the old device in the web dashboard. Reset the unavailable local identity now?")) void action("reset_identity");
});
for (const id of ["autostart", "minimize"]) element(id).addEventListener("change", () => void action("preferences", { autostart: element<HTMLInputElement>("autostart").checked, minimize: element<HTMLInputElement>("minimize").checked }));
element("onboarding-autostart").addEventListener("change", () => void action("preferences", { autostart: element<HTMLInputElement>("onboarding-autostart").checked, minimize: element<HTMLInputElement>("minimize").checked }));
void refresh(); setInterval(() => void refresh(), 2000);
