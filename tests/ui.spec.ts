import { test, expect, type Page } from "@playwright/test";
import type { Status } from "../src/ui";
import { receivedText, cloudProblemText } from "../src/ui";

const healthy: Status = {
  pairing: {
    device_id: "qa-device",
    channel: {
      twitch_id: "123",
      username: "necko",
      display_name: "Necko",
      avatar_url: null,
    },
  },
  minimize_to_tray: true,
  autostart: false,
  gsi: "GSI config installed",
  listener: "Listening on 127.0.0.1:31338",
  cloud: "Connected",
  gsi_active: false,
  last_gsi_at: null,
  forwarding_error: null,
  pairing_code: "",
  pairing_busy: false,
  error: null,
  identity_error: null,
  version: "0.2.0",
};
async function fixture(page: Page, changes: Partial<Status> = {}) {
  await page.addInitScript(
    ({ status }) => {
      const qa = window as unknown as {
        qaStatus: Status;
        qaCalls: { command: string; args: Record<string, unknown> }[];
        __TAURI_INTERNALS__: unknown;
        qaRejectCommand?: string;
        qaFailure?: string;
      };
      qa.qaStatus = status;
      qa.qaCalls = [];
      qa.__TAURI_INTERNALS__ = {
        invoke: async (command: string, args: Record<string, unknown>) => {
          if (command === "status") return qa.qaStatus;
          qa.qaCalls.push({ command, args });
          if (command === qa.qaRejectCommand) throw qa.qaFailure;
          if (command === "pair") {
            await new Promise((resolve) => setTimeout(resolve, 600));
            qa.qaStatus.pairing = {
              device_id: "qa-device",
              channel: {
                twitch_id: "123",
                username: "necko",
                display_name: "Necko",
                avatar_url: null,
              },
            };
          }
          if (command === "preferences") {
            qa.qaStatus.autostart = !!args.autostart;
            qa.qaStatus.minimize_to_tray = !!args.minimize;
          }
          if (command === "unpair") qa.qaStatus.pairing = null;
        },
      };
    },
    { status: { ...structuredClone(healthy), ...changes } },
  );
  await page.route(/^https?:\/\/(?!127\.0\.0\.1|localhost)/, (route) =>
    route.abort(),
  );
  await page.goto("/");
  await expect(page.locator("#settings-button")).toBeVisible();
}
async function screenshot(page: Page, state: string) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(360);
  const content = page.locator(".companion-content");
  expect(await content.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  if (!await page.locator("#settings").isVisible()) {
    await expect(page.locator("#settings-button")).toBeInViewport();
    if (await page.locator("#pairing").isVisible()) await expect(page.locator("#onboarding-autostart")).toBeInViewport();
  }
  const dialog = page.locator("#settings");
  if (await dialog.isVisible()) expect(await dialog.evaluate(el => el.scrollHeight <= el.clientHeight)).toBe(true);
  if (state === "revoked" || state === "pairing-error") {
    const error = await page.locator(state === "revoked" ? "#cloud" : "#error").boundingBox();
    const contentBox = await content.boundingBox();
    expect(error!.y + error!.height).toBeLessThanOrEqual(contentBox!.y + contentBox!.height);
  }
  await page.screenshot({ path: `../.qa/companion-redesign/final-${state}-browser.png` });
}
test("healthy paired state is compact and local activity is independent of cloud", async ({
  page,
}) => {
  await fixture(page, {
    gsi_active: true,
    last_gsi_at: new Date().toISOString(),
  });
  await expect(page.getByText("CS2 Active", { exact: true })).toBeVisible();
  await expect(page.getByText("Receiving GSI · just now")).toBeVisible();
  await expect(page.locator("#cloud-issue")).toBeHidden();
  await page.getByRole("button", { name: "Open Dashboard" }).click();
  await page.getByRole("button", { name: /Necko/ }).click();
  expect(
    await page.evaluate(() =>
      (window as any).qaCalls.map((r: any) => r.command),
    ),
  ).toEqual(["open_dashboard", "open_channel"]);
  await screenshot(page, "active");
});
test("unpaired flow prevents duplicate pairing and becomes paired", async ({
  page,
}) => {
  await fixture(page, { pairing: null });
  await expect(
    page.getByRole("heading", { name: "Connect your channel" }),
  ).toBeVisible();
  await expect(page.locator("#activity")).toBeHidden();
  await screenshot(page, "unpaired");
  await page.getByLabel("Pairing code").fill("ABCD-2345");
  await page.getByRole("button", { name: "Connect channel" }).click();
  await expect(
    page.getByRole("button", { name: "Connecting..." }),
  ).toBeDisabled();
  await screenshot(page, "busy");
  await expect(page.locator("#channel")).toBeVisible();
  expect(
    await page.evaluate(
      () =>
        (window as any).qaCalls.filter((r: any) => r.command === "pair").length,
    ),
  ).toBe(1);
});
test("never received and stale activity remain truthful", async ({ page }) => {
  await fixture(page);
  await expect(
    page.getByText("Waiting for first game update"),
  ).toBeVisible();
  await screenshot(page, "never-received");
  await page.evaluate(() => {
    (window as any).qaStatus.last_gsi_at = new Date(
      Date.now() - 61000,
    ).toISOString();
  });
  await expect(page.getByText("Last GSI · 1m ago")).toBeVisible();
  await expect(
    page.getByText("CS2 Not active", { exact: true }),
  ).toBeVisible();
  await screenshot(page, "stale");
});
test("settings trap focus, close with Escape and restore focus without changing dimensions", async ({
  page,
}) => {
  await fixture(page);
  const before = await page.locator("#app").boundingBox();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("Version 0.2.0")).toBeVisible();
  for (let i = 0; i < 8; i++) {
    await page.keyboard.press("Tab");
    expect(
      await page.evaluate(() => !!document.activeElement?.closest("dialog")),
    ).toBe(true);
  }
  await screenshot(page, "settings");
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await expect(
    page.getByRole("button", { name: "Settings", exact: true }),
  ).toBeFocused();
  expect(await page.locator("#app").boundingBox()).toEqual(before);
});
test("settings unpair is explicit and cancelling has no side effect", async ({
  page,
}) => {
  await fixture(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page
    .getByRole("button", { name: "Unpair desktop", exact: true })
    .click();
  await expect(
    page.getByText("Stop forwarding game updates for this channel?"),
  ).toBeVisible();
  await screenshot(page, "unpair");
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  expect(await page.evaluate(() => (window as any).qaCalls.length)).toBe(0);
  await page
    .getByRole("button", { name: "Unpair desktop", exact: true })
    .click();
  await page.locator("#confirm-action").click();
  await expect(
    page.getByRole("heading", { name: "Connect your channel" }),
  ).toBeVisible();
  await expect(page.getByRole("dialog")).not.toBeVisible();
});
test("actionable configuration and forwarding failures are visible without hiding local GSI", async ({
  page,
}) => {
  await fixture(page, {
    gsi: "GSI config missing — Retry discovery",
    gsi_active: true,
    last_gsi_at: new Date().toISOString(),
    forwarding_error: "Cloud unavailable or timed out",
    cloud: "Desktop connected",
  });
  await expect(page.getByText("CS2 Active", { exact: true })).toBeVisible();
  await expect(page.getByText("Can't send game updates. Check your connection.")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Retry discovery" }),
  ).toBeVisible();
  await expect(page.locator("#gsi")).toHaveText("GSI config missing");
  await page.getByRole("button", { name: "Retry discovery" }).click();
  expect(await page.evaluate(() => (window as any).qaCalls[0].command)).toBe("retry_discovery");
  await screenshot(page, "combined-errors");
});
test("broken avatars keep a fallback after status polling and long names fit", async ({
  page,
}) => {
  await fixture(page, {
    pairing: {
      ...healthy.pairing!,
      channel: {
        ...healthy.pairing!.channel,
        display_name: "A very long channel display name with many characters",
        username: "a_very_long_twitch_channel_name",
        twitch_id: "123456789012",
        avatar_url: "https://example.test/avatar.png",
      },
    },
  });
  await expect(page.locator("#avatar-fallback")).toBeVisible();
  await page.waitForTimeout(2200);
  await expect(page.locator("#avatar-fallback")).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(
    360,
  );
  await screenshot(page, "long-avatar-failure");
});

test("configuration, listener, cloud, and revoked states stay actionable", async ({ page }) => {
  await fixture(page, { gsi: "GSI config missing — Retry discovery" });
  await screenshot(page, "configuration");
  await page.evaluate(() => {
    (window as any).qaStatus.gsi = "GSI config installed";
    (window as any).qaStatus.listener = "Listener unavailable — port in use. Close the conflicting app and restart.";
  });
  await expect(page.locator("#gsi")).toContainText("Listener unavailable");
  await expect(page.locator("#retry")).toBeHidden();
  await screenshot(page, "listener");
  await page.evaluate(() => {
    (window as any).qaStatus.listener = "Listening";
    (window as any).qaStatus.forwarding_error = "Cloud unavailable or timed out";
  });
  await expect(page.locator("#discovery-issue")).toBeHidden();
  await expect(page.locator("#cloud")).toHaveText("Can't send game updates. Check your connection.");
  await screenshot(page, "cloud");
  await page.evaluate(() => {
    (window as any).qaStatus.forwarding_error = "Cloud rejected GSI (503); check clock and backend";
  });
  await expect(page.locator("#cloud")).toHaveText("Game updates were rejected (503). Check the dashboard.");
  await expect(page.locator("#cloud")).toHaveAttribute("title", "Cloud rejected GSI (503); check clock and backend");
  await screenshot(page, "cloud-rejected");
  await page.evaluate(() => {
    (window as any).qaStatus.pairing = null;
    (window as any).qaStatus.forwarding_error = null;
    (window as any).qaStatus.cloud = "Device revoked — pair again";
  });
  await expect(page.locator("#cloud")).toHaveText("Device revoked — pair again");
  await screenshot(page, "revoked");
});

test("freshness wording preserves the independent activity contract", () => {
  const now = Date.parse("2026-09-28T12:00:00Z");
  expect(receivedText(null, now)).toBe("Waiting for first game update");
  expect(receivedText(new Date(now - 1000).toISOString(), now, true)).toBe("Receiving GSI · just now");
  expect(receivedText(new Date(now - 43000).toISOString(), now, true)).toBe("Receiving GSI · 43s ago");
  expect(receivedText(new Date(now - 60000).toISOString(), now)).toBe("Last GSI · 1m ago");
  expect(receivedText(new Date(now - 7200000).toISOString(), now)).toBe("Last GSI · 2h ago");
  expect(receivedText("invalid", now)).toBe("Last update time unavailable");
  expect(cloudProblemText("Cloud rejected GSI (503); check clock and backend")).toBe("Game updates were rejected (503). Check the dashboard.");
  expect(cloudProblemText("Device revoked — pair again")).toBe("Device revoked — pair again");
  expect(cloudProblemText("Unknown failure with meaningful details")).toBe("Unknown failure with meaningful details");
});

test("retry failures show one problem and retain full diagnostic state", async ({ page }) => {
  const failure = "Cannot write CS2 GSI config; check folder permissions";
  await fixture(page, { gsi: failure });
  await page.evaluate(message => {
    (window as any).qaRejectCommand = "retry_discovery";
    (window as any).qaFailure = message;
  }, failure);
  await page.getByRole("button", { name: "Retry discovery" }).click();
  await expect(page.locator("#gsi")).toHaveText(failure);
  await expect(page.locator("#error")).toBeHidden();
  await expect(page.locator("#gsi")).toHaveAttribute("title", failure);
  await screenshot(page, "discovery-failure");
});

test("preferences, pairing errors, and backend-busy state keep protections", async ({ page }) => {
  await fixture(page);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("dialog").getByLabel("Start with Windows", { exact: true }).check();
  await page.getByLabel("Minimize to tray when closing").uncheck();
  expect(await page.evaluate(() => (window as any).qaStatus.autostart)).toBe(true);
  expect(await page.evaluate(() => (window as any).qaStatus.minimize_to_tray)).toBe(false);
  await page.keyboard.press("Escape");
  await page.evaluate(() => {
    (window as any).qaStatus.pairing = null;
    (window as any).qaStatus.pairing_busy = true;
    (window as any).qaStatus.pairing_code = "ABCD-2345";
  });
  await expect(page.getByRole("button", { name: "Connecting..." })).toBeDisabled();
  await page.evaluate(() => {
    (window as any).qaStatus.pairing_busy = false;
    (window as any).qaStatus.error = "Pairing code invalid or expired. Get a new code in the dashboard.";
  });
  await expect(page.locator("#error")).toContainText("Pairing code invalid or expired");
  await screenshot(page, "pairing-error");
});

test("available HTTPS avatars render and non-HTTPS avatars use the fallback", async ({ page }) => {
  await fixture(page);
  await page.route("https://example.test/avatar.png", route => route.fulfill({ path: "src-tauri/icons/128x128.png", contentType: "image/png" }));
  await page.evaluate(() => {
    (window as any).qaStatus.pairing.channel.avatar_url = "https://example.test/avatar.png";
  });
  await expect(page.locator("#avatar")).toBeVisible();
  expect(await page.locator("#avatar").evaluate(el => (el as HTMLImageElement).naturalWidth)).toBe(128);
  await expect(page.locator("#avatar-fallback")).toBeHidden();
  await screenshot(page, "avatar-loaded");
  await page.evaluate(() => {
    (window as any).qaStatus.pairing.channel.avatar_url = "http://example.test/avatar.png";
  });
  await expect(page.locator("#avatar")).toBeHidden();
  await expect(page.locator("#avatar-fallback")).toBeVisible();
});

for (const scale of [1.25, 1.5, 2]) {
  test(`fixed shell and modal fit at ${scale * 100}% raster scale`, async ({ browser }) => {
    const context = await browser.newContext({ viewport: { width: 360, height: 280 }, deviceScaleFactor: scale });
    const page = await context.newPage();
    await fixture(page, { gsi_active: true, last_gsi_at: new Date().toISOString() });
    await screenshot(page, `scale-${scale * 100}-active`);
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await screenshot(page, `scale-${scale * 100}-settings`);
    await page.getByRole("button", { name: "Unpair desktop", exact: true }).click();
    await screenshot(page, `scale-${scale * 100}-unpair`);
    await page.keyboard.press("Tab");
    await page.keyboard.press("Tab");
    expect(await page.evaluate(() => !!document.activeElement?.closest("dialog"))).toBe(true);
    await page.keyboard.press("Escape");
    await expect(page.getByRole("button", { name: "Settings", exact: true })).toBeFocused();
    await context.close();
  });
}
