import { test, expect } from "@playwright/test";
import { guest, owner } from "./cafe.mjs";

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

  // The booth is an opaque scene; half-lighting it over the hero photo
  // double-exposed the page.
  test("the booth canvas is not blended over the page background", async ({ page }) => {
    await page.goto("/");
    const opacity = await page
      .locator("#cafe-3d")
      .evaluate((el) => parseFloat(getComputedStyle(el).opacity));
    expect(opacity).toBe(1);
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

  test("a wrong owner pin is refused and the door stays shut", async ({ page }) => {
    await page.goto("/");
    await page.getByTestId("owner-pin").fill("nope");
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("door-error")).toContainText("pin");
    await expect(page.getByTestId("stage-door")).toBeVisible();
    await expect(page.getByTestId("stage-app")).toBeHidden();
  });

  test("an empty owner pin is refused", async ({ page }) => {
    await page.goto("/");
    await page.getByTestId("owner-pin").fill("");
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("door-error")).toContainText("pin");
    await expect(page.getByTestId("stage-app")).toBeHidden();
  });

  test("a guest who types no name still gets in", async ({ page }) => {
    await page.goto("/");
    await page.getByTestId("guest-name").fill("");
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await expect(page.getByTestId("role-label")).toHaveText("guest");
  });

  test("a guest arrives with the 50 USDC grant", async ({ page }) => {
    await guest(page);
    await expect(page.getByTestId("balance")).toHaveText("50");
  });

  test("the owner arrives with no till of their own", async ({ page }) => {
    await owner(page);
    await expect(page.getByTestId("balance")).toHaveText("0");
    await expect(page.getByTestId("owner-tools")).toBeVisible();
  });
});
