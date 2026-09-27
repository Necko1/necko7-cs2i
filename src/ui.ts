export interface Status {
  pairing: { device_id: string; channel: { twitch_id: string; username: string; display_name: string; avatar_url: string | null } } | null;
  minimize_to_tray: boolean; autostart: boolean; gsi: string; listener: string; cloud: string;
  pairing_code: string; pairing_busy: boolean; error: string | null; identity_error: string | null; version: string;
}
export function element<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id); if (!el) throw new Error(`Missing UI element: ${id}`); return el as T;
}
export function render(status: Status) {
  element("pairing").hidden = !!status.pairing;
  element("channel").hidden = !status.pairing;
  element("connection").textContent = status.pairing ? "Channel paired" : "Connect your Twitch channel";
  element("gsi").textContent = status.gsi;
  element("listener").textContent = status.listener;
  element("cloud").textContent = status.cloud;
  element("version").textContent = `Version ${status.version}`;
  element<HTMLInputElement>("autostart").checked = status.autostart;
  element<HTMLInputElement>("onboarding-autostart").checked = status.autostart;
  element<HTMLInputElement>("minimize").checked = status.minimize_to_tray;
  element<HTMLButtonElement>("connect").disabled = status.pairing_busy || !!status.identity_error;
  element("connect").textContent = status.pairing_busy ? "Connecting…" : "Connect";
  element("unpair").hidden = !status.pairing;
  element("reset").hidden = !status.identity_error;
  element("identity").textContent = status.identity_error ?? "";
  const code = element<HTMLInputElement>("code");
  if (status.pairing_code && document.activeElement !== code) code.value = status.pairing_code;
  if (status.pairing) {
    const { channel } = status.pairing;
    element("display-name").textContent = channel.display_name;
    element("username").textContent = `@${channel.username}`;
    element("twitch-id").textContent = `Twitch ID: ${channel.twitch_id}`;
    const avatar = element<HTMLImageElement>("avatar");
    const url = channel.avatar_url && /^https:\/\//i.test(channel.avatar_url) ? channel.avatar_url : null;
    avatar.hidden = !url; if (url && avatar.src !== url) avatar.src = url;
  }
}
