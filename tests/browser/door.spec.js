import { test, expect } from "@playwright/test";
import { guest, owner, GRANT } from "./cafe.mjs";

test.describe("the door", () => {
  test("offers both doors and names the cafe", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("login-guest")).toBeVisible();
    await expect(page.getByTestId("login-owner")).toBeVisible();
    await expect(page.getByRole("heading", { name: "Causewaybay Coffee" })).toBeVisible();
    await expect(page.getByText("銅鑼灣咖啡")).toBeVisible();
    await expect(page.getByTestId("stage-app")).toBeHidden();
  });

  // The card once had no background of its own, so the hero photo and the 3-D
  // booth showed straight through the form and nothing was readable.
  test("the card sits on a solid panel, not on the artwork", async ({ page }) => {
    await page.goto("/");
    const alpha = await page.locator(".door-card").evaluate((el) => {
      const bg = getComputedStyle(el).backgroundColor;
      const m = bg.match(/rgba?\(([^)]+)\)/);
      if (!m) return 0;
      const parts = m[1].split(",").map((n) => parseFloat(n));
      return parts.length > 3 ? parts[3] : 1;
    });
    expect(alpha).toBeGreaterThan(0.85);
  });

  // A shop on the cafe's own wifi must not depend on the internet to draw
  // itself: no CDN scripts, no Google Fonts, everything served from here.
  test("the page loads nothing from outside the shop", async ({ page }) => {
    const external = [];
    page.on("request", (r) => {
      const u = new URL(r.url());
      if (u.hostname !== "127.0.0.1" && u.hostname !== "localhost") external.push(r.url());
    });
    await page.goto("/");
    await expect(page.getByTestId("login-guest")).toBeVisible();
    await page.waitForTimeout(500);
    expect(external).toEqual([]);
    // And the typefaces really are ours.
    const served = await page.evaluate(async () => {
      await document.fonts.ready;
      return [...document.fonts].filter((f) => f.status === "loaded").map((f) => f.family);
    });
    expect(served.some((f) => /Sora/.test(f))).toBeTruthy();
  });

  test("the mascot plate is knocked out to transparent", async ({ page }) => {
    await page.goto("/");
    const corner = await page.locator("#mascot").evaluate(async (c) => {
      for (let i = 0; i < 60; i++) {
        const d = c.getContext("2d").getImageData(2, 2, 1, 1).data;
        if (d[3] === 0) return { alpha: 0 };
        await new Promise((r) => setTimeout(r, 100));
      }
      const d = c.getContext("2d").getImageData(2, 2, 1, 1).data;
      return { alpha: d[3] };
    });
    expect(corner.alpha).toBe(0);
  });

  // A simulation has no till worth locking, so the demo is never stuck at
  // the door. The live project checks that a real shop still refuses.
  test("in a simulation any pin opens the counter, and the door says so", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("local-note")).toContainText("any pin opens the counter");
    for (const pin of ["nope", ""]) {
      await page.goto("/");
      await page.getByTestId("owner-pin").fill(pin);
      await page.getByTestId("login-owner").click();
      await expect(page.getByTestId("stage-app")).toBeVisible();
      await expect(page.getByTestId("role-label")).toHaveText("owner");
      await page.getByTestId("leave").click();
    }
  });

  test("a guest who types no name still gets in", async ({ page }) => {
    await page.goto("/");
    await page.getByTestId("guest-name").fill("");
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await expect(page.getByTestId("role-label")).toHaveText("guest");
  });

  test("a guest arrives with a purse of test money", async ({ page }) => {
    await guest(page);
    await expect(page.getByTestId("balance")).toHaveText(GRANT);
    await expect(page.getByTestId("purse-label")).toHaveText("Causewaybay Coin");
  });

  test("the shop says which money it is taking", async ({ page }) => {
    await guest(page);
    const badge = page.getByTestId("mode-badge");
    await expect(badge).toBeVisible();
    await expect(badge).toContainText("Test money");
    await expect(badge).toContainText("Causewaybay Coin");
    await expect(badge).toContainText("HKD");
  });

  test("the owner keeps no purse of their own", async ({ page }) => {
    await owner(page);
    await expect(page.getByTestId("owner-tools")).toBeVisible();
    await expect(page.getByTestId("purse-label")).toBeHidden();
    await expect(page.getByTestId("faucet")).toBeHidden();
  });
});
