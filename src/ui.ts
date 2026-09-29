export interface Status {
  pairing: {
    device_id: string;
    channel: {
      twitch_id: string;
      username: string;
      display_name: string;
      avatar_url: string | null;
    };
  } | null;
  minimize_to_tray: boolean;
  autostart: boolean;
  gsi: string;
  listener: string;
  cloud: string;
  pairing_code: string;
  pairing_busy: boolean;
  error: string | null;
  error_scope?: "pairing" | "persistence" | null;
  identity_error: string | null;
  version: string;
  gsi_active: boolean;
  last_gsi_at: string | null;
  forwarding_error: string | null;
}
export function element<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) throw new Error(`Missing UI element: ${id}`);
  return el as T;
}
export function render(status: Status) {
  element("pairing").hidden = !!status.pairing;
  element("channel").hidden = !status.pairing;
  element("onboarding-preference").hidden = !!status.pairing;
  element("connection").textContent = status.pairing
    ? "Channel paired"
    : "Connect your channel";
  element("activity").dataset.active = String(status.gsi_active);
  element("activity").hidden = !status.pairing;
  element("activity-title").textContent = status.gsi_active
    ? "CS2 Active"
    : "CS2 Not active";
  element("last-gsi").textContent = receivedText(
    status.last_gsi_at,
    Date.now(),
    status.gsi_active,
  );
  const configHealthy = status.gsi.startsWith("GSI config installed");
  const listenerHealthy =
    status.listener.startsWith("Listening") ||
    status.listener.startsWith("Receiving");
  element("discovery-issue").hidden = configHealthy && listenerHealthy;
  element("retry").hidden = configHealthy;
  const discoveryError = !configHealthy
    ? status.gsi.replace(/\s*—\s*Retry discovery$/, "")
    : !listenerHealthy
      ? status.listener
      : "";
  element("gsi").textContent = discoveryError;
  element("gsi").title = !configHealthy ? status.gsi : status.listener;
  const cloudHealthy = [
    "Connected",
    "Desktop connected",
    "Waiting for GSI",
    "Paired — checking desktop connection",
    "Unpaired",
  ].includes(status.cloud);
  const cloudError =
    status.forwarding_error ?? (!cloudHealthy ? status.cloud : null);
  element("cloud-issue").hidden = !cloudError;
  element("cloud").textContent = cloudError ? cloudProblemText(cloudError) : "";
  element("cloud").title = cloudError ?? "";
  element("version").textContent = `Version ${status.version}`;
  element<HTMLInputElement>("autostart").checked = status.autostart;
  element<HTMLInputElement>("onboarding-autostart").checked = status.autostart;
  element<HTMLInputElement>("minimize").checked = status.minimize_to_tray;
  element<HTMLButtonElement>("connect").disabled =
    status.pairing_busy || !!status.identity_error;
  element("connect-label").textContent = status.pairing_busy
    ? "Connecting..."
    : "Connect channel";
  element("dashboard").hidden = !status.pairing;
  element("unpair").hidden = !status.pairing;
  element("reset").hidden = !status.identity_error;
  element("identity").textContent = status.identity_error ?? "";
  const code = element<HTMLInputElement>("code");
  if (status.pairing_code && document.activeElement !== code)
    code.value = status.pairing_code;
  if (status.pairing) {
    const { channel } = status.pairing;
    element("display-name").textContent = channel.display_name;
    element("username").textContent = `@${channel.username}`;
    element("twitch-id").textContent = `ID ${channel.twitch_id}`;
    element("twitch-id").title = `Twitch/channel ID: ${channel.twitch_id}`;
    element("channel-link").title = `Open @${channel.username} on Twitch (${channel.twitch_id})`;
    element("display-name").title = channel.display_name;
    element("avatar-fallback").textContent = channel.display_name
      .slice(0, 2)
      .toUpperCase();
    const avatar = element<HTMLImageElement>("avatar");
    const url =
      channel.avatar_url && /^https:\/\//i.test(channel.avatar_url)
        ? channel.avatar_url
        : null;
    const failed = avatar.dataset.failed === url;
    avatar.hidden = !url || failed;
    element("avatar-fallback").hidden = !!url && !failed;
    if (url && avatar.src !== url) avatar.src = url;
  }
}
export function cloudProblemText(reason: string) {
  if (reason === "Cloud unavailable or timed out")
    return "Can't send game updates. Check your connection.";
  const rejected = /^Cloud rejected GSI \((\d{3})\); check clock and backend$/.exec(reason);
  if (rejected)
    return `Game updates were rejected (${rejected[1]}). Check the dashboard.`;
  return reason;
}
export function receivedText(value: string | null, now = Date.now(), active = false) {
  if (!value) return "Waiting for first game update";
  const age = Math.max(0, (now - Date.parse(value)) / 1000);
  if (!Number.isFinite(age)) return "Last update time unavailable";
  const label = active ? "Receiving GSI" : "Last GSI";
  if (age < 5) return `${label} · just now`;
  if (age < 60) return `${label} · ${Math.floor(age)}s ago`;
  const minutes = Math.floor(age / 60);
  return minutes < 60
    ? `${label} · ${minutes}m ago`
    : `${label} · ${Math.floor(minutes / 60)}h ago`;
}
