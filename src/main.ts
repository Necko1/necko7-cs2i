import { invoke } from "@tauri-apps/api/core";
import {
  createIcons,
  Settings,
  X,
  ExternalLink,
  Gamepad2,
  CircleAlert,
  Link,
  RefreshCw,
} from "lucide";
import { element, render, type Status } from "./ui";
import "./styles.css";

element("app").innerHTML = `
<div class="companion-content">
<div id="pairing"><h1 id="connection">Connect your channel</h1><p class="pairing-guidance muted">Get a pairing code from <strong>Scripts &gt; CS2 Integration</strong> in your dashboard.</p><form id="pair-form"><label for="code">Pairing code</label><input id="code" maxlength="9" placeholder="7K3M-PQ8D" autocomplete="off" spellcheck="false" required><button id="connect" type="submit"><i data-lucide="link"></i><span id="connect-label">Connect channel</span></button></form></div>
<div id="channel" hidden><div class="avatar"><img id="avatar" alt="Channel avatar" referrerpolicy="no-referrer"><span id="avatar-fallback"></span></div><div class="channel-identity"><button id="channel-link" title="Open Twitch channel"><strong id="display-name"></strong><span class="channel-meta"><span id="username"></span><span class="meta-divider" aria-hidden="true">&middot;</span><span id="twitch-id"></span></span></button></div></div>
<section id="activity" data-active="false" aria-labelledby="activity-title"><div class="activity-icon" aria-hidden="true"><i data-lucide="gamepad-2"></i></div><div class="activity-copy"><h2 id="activity-title">CS2 Not active</h2><p id="last-gsi" class="muted">Waiting for first game update</p></div></section>
<div class="issues"><p id="error" role="alert"></p><p id="identity" role="alert"></p><div id="discovery-issue" class="issue"><i data-lucide="circle-alert" aria-hidden="true"></i><div><p id="gsi"></p><button id="retry" class="text-button"><i data-lucide="refresh-cw"></i>Retry discovery</button></div></div><div id="cloud-issue" class="issue" hidden><i data-lucide="circle-alert" aria-hidden="true"></i><p id="cloud" role="status"></p></div></div>
</div>
<footer><label id="onboarding-preference" class="toggle onboarding"><input id="onboarding-autostart" type="checkbox">Start with Windows</label><button id="dashboard" hidden><i data-lucide="external-link"></i>Open Dashboard</button><button id="settings-button" class="secondary" title="Settings" aria-haspopup="dialog"><i data-lucide="settings"></i>Settings</button></footer>
<dialog id="settings" aria-labelledby="settings-title"><div class="dialog-header"><h2 id="settings-title">Settings</h2><button id="settings-close" class="icon-button" aria-label="Close settings" title="Close settings"><i data-lucide="x"></i></button></div><div class="settings-preferences"><label class="toggle"><input id="autostart" type="checkbox">Start with Windows</label><label class="toggle"><input id="minimize" type="checkbox">Minimize to tray when closing</label></div><p id="settings-error" role="alert"></p><div class="settings-bottom"><p id="version" class="muted"></p><button id="unpair" class="danger text-button">Unpair desktop</button><button id="reset" class="danger text-button" hidden>Reset unavailable identity</button></div><div id="settings-confirm" hidden><h3 id="confirm-title">Unpair desktop?</h3><p id="confirm-message"></p><div class="confirmation-actions"><button id="confirm-cancel" class="secondary">Cancel</button><button id="confirm-action" class="danger">Unpair desktop</button></div></div></dialog>`;
createIcons({
  icons: { Settings, X, ExternalLink, Gamepad2, CircleAlert, Link, RefreshCw },
});
element<HTMLImageElement>("avatar").addEventListener("error", () => {
  const avatar = element<HTMLImageElement>("avatar");
  avatar.dataset.failed = avatar.src;
  avatar.hidden = true;
  element("avatar-fallback").hidden = false;
});
const settings = element<HTMLDialogElement>("settings");
function showError(message: string, surface: "main" | "settings" = "main") {
  const duplicate =
    !settings.open &&
    !!message &&
    [element("gsi").title, element("cloud").title, element("identity").textContent].includes(message);
  if (duplicate) message = "";
  element(surface === "settings" ? "settings-error" : "error").textContent = message;
}
let busy = false;
const localErrors = new Map<string, string>();
let confirmCommand: string | null = null;
async function refresh() {
  if (busy) return;
  try {
    const status = await invoke<Status>("status");
    render(status);
    if (status.pairing_busy && settings.open) settings.close();
    if (status.pairing && !status.pairing_busy) localErrors.delete("pair");
    const pairingError = !status.pairing && status.error_scope !== "persistence" ? status.error : null;
    const mainError = [...localErrors].find(([command]) =>
      !["preferences", "unpair", "reset_identity"].includes(command) ||
      (command === "preferences" && !status.pairing && !settings.open))?.[1];
    const settingsError = [...localErrors].find(([command]) => ["preferences", "unpair", "reset_identity"].includes(command))?.[1];
    showError(pairingError ?? (status.error_scope === "persistence" ? status.error : null) ?? mainError ?? "");
    showError(settingsError ?? status.startup_error ?? "", "settings");
  } catch {
    showError("Desktop services unavailable. Restart the app.");
  }
}
async function action(command: string, args?: Record<string, unknown>) {
  if (busy) return;
  busy = true;
  localErrors.delete(command);
  document
    .querySelectorAll<HTMLButtonElement | HTMLInputElement>("button,input")
    .forEach((el) => (el.disabled = true));
  if (command === "pair")
    element("connect-label").textContent = "Connecting...";
  try {
    await invoke(command, args);
    if (command === "unpair" || command === "reset_identity") {
      element<HTMLInputElement>("code").value = "";
      settings.close();
    }
  } catch (error) {
    localErrors.set(command, String(error));
  } finally {
    busy = false;
    document
      .querySelectorAll<HTMLButtonElement | HTMLInputElement>("button,input")
      .forEach((el) => (el.disabled = false));
    await refresh();
  }
}
element("pair-form").addEventListener("submit", (event) => {
  event.preventDefault();
  void action("pair", { code: element<HTMLInputElement>("code").value });
});
element("settings-button").addEventListener("click", () => {
  element("settings-confirm").hidden = true;
  settings.showModal();
});
element("settings-close").addEventListener("click", () => settings.close());
settings.addEventListener("cancel", (event) => {
  if (busy) event.preventDefault();
});
settings.addEventListener("keydown", (event) => {
  if (event.key !== "Tab") return;
  const controls = Array.from(
    settings.querySelectorAll<HTMLElement>(
      "button:not(:disabled),input:not(:disabled)",
    ),
  ).filter((control) => control.getClientRects().length > 0);
  const target = event.shiftKey ? controls.at(-1) : controls[0];
  if (
    !controls.length ||
    (!event.shiftKey && document.activeElement === controls.at(-1)) ||
    (event.shiftKey && document.activeElement === controls[0])
  ) {
    event.preventDefault();
    target?.focus();
  }
});
settings.addEventListener("click", (event) => {
  if (event.target === settings && !busy) {
    const r = settings.getBoundingClientRect();
    if (
      event.clientX < r.left ||
      event.clientX > r.right ||
      event.clientY < r.top ||
      event.clientY > r.bottom
    )
      settings.close();
  }
});
settings.addEventListener("close", () => element("settings-button").focus());
element("retry").addEventListener(
  "click",
  () => void action("retry_discovery"),
);
element("dashboard").addEventListener(
  "click",
  () => void action("open_dashboard"),
);
element("channel-link").addEventListener(
  "click",
  () => void action("open_channel"),
);
function confirm(command: string) {
  confirmCommand = command;
  element("settings-confirm").hidden = false;
  element("confirm-title").textContent = command === "unpair" ? "Unpair desktop?" : "Reset identity?";
  element("confirm-message").textContent =
    command === "unpair"
      ? "Stop forwarding game updates for this channel? You can pair again with a new dashboard code."
      : "Revoke the old device in the dashboard before resetting this unavailable identity.";
  element("confirm-action").textContent =
    command === "unpair" ? "Unpair desktop" : "Reset identity";
  element("confirm-cancel").focus();
}
element("unpair").addEventListener("click", () => confirm("unpair"));
element("reset").addEventListener("click", () => confirm("reset_identity"));
element("confirm-cancel").addEventListener("click", () => {
  element("settings-confirm").hidden = true;
  element(confirmCommand === "unpair" ? "unpair" : "reset").focus();
});
element("confirm-action").addEventListener("click", () => {
  if (confirmCommand) void action(confirmCommand);
});
for (const id of ["autostart", "minimize"])
  element(id).addEventListener(
    "change",
    () =>
      void action("preferences", {
        autostart: element<HTMLInputElement>("autostart").checked,
        minimize: element<HTMLInputElement>("minimize").checked,
      }),
  );
element("onboarding-autostart").addEventListener(
  "change",
  () =>
    void action("preferences", {
      autostart: element<HTMLInputElement>("onboarding-autostart").checked,
      minimize: element<HTMLInputElement>("minimize").checked,
    }),
);
void refresh();
setInterval(() => void refresh(), 2000);
