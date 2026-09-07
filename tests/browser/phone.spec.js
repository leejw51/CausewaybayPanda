import { test, expect } from "@playwright/test";
import { guest, owner, say, dish, cartLine, payAndNumber, myOrder, ticket } from "./cafe.mjs";

// Every other spec runs at 1280×720, where the ticket is a panel and Pay is
// always on screen. On a phone the ticket is a sheet resting on the dock and
// Pay lives inside its body, so a cart the guest never opened is a cart the
// guest cannot pay. That is the flow README leads with.
test.describe("a guest on a phone", () => {
  test("ordering by chat leaves Pay reachable", async ({ page }) => {
    await guest(page);
    await say(page, "two lattes and an egg tart");
    await expect(cartLine(page, "latte")).toContainText("2×");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$86.00");
    await expect(page.getByTestId("pay-usdc")).toBeVisible();
  });

  test("ordering by tap leaves Pay reachable", async ({ page }) => {
    await guest(page);
    await dish(page, "egg_tart").click();
    await expect(cartLine(page, "egg_tart")).toContainText("1×");
    await expect(page.getByTestId("pay-usdc")).toBeVisible();
  });

  test("the sheet summary reads the cart back while it is shut", async ({ page }) => {
    await guest(page);
    await say(page, "latte");
    await expect(page.getByTestId("pay-usdc")).toBeVisible();
    await page.getByTestId("sheet-toggle").click();
    await expect(page.getByTestId("sheet-summary")).toContainText("1 item");
    await expect(page.getByTestId("sheet-summary")).toContainText("HK$38.00");
  });

  // A sheet the guest deliberately shut stays shut while they only change
  // quantities — it opens for a first line, not for every cart frame.
  test("closing the sheet is not undone by changing a quantity", async ({ page }) => {
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("sheet-toggle").click();
    await expect(page.getByTestId("pay-usdc")).toBeHidden();
    await dish(page, "latte").click();
    await expect(page.getByTestId("sheet-summary")).toContainText("2 items");
    await expect(page.getByTestId("pay-usdc")).toBeHidden();
  });

  test("a phone guest pays and watches the card to ready", async ({ page }) => {
    await guest(page);
    await say(page, "an egg tart");
    const no = await payAndNumber(page);
    await expect(myOrder(page, no)).toBeVisible();
  });

  // A paid cart is empty again, so the next order is a first line: the
  // sheet comes back for it.
  test("after paying, the next order opens the sheet again", async ({ page }) => {
    await guest(page);
    await say(page, "latte");
    await payAndNumber(page);
    await expect(page.getByTestId("pay-usdc")).toBeHidden();
    await dish(page, "egg_tart").click();
    await expect(page.getByTestId("pay-usdc")).toBeVisible();
  });
});

// The owner's phone is the kitchen. The tools come first and the queue's
// buttons are thumb-sized; nothing here needs a laptop.
test.describe("the owner on a phone", () => {
  test("the tools lead and a ticket is worked from the queue", async ({ page, browser }) => {
    const shop = await owner(page);
    await expect(shop.getByTestId("owner-tools")).toBeVisible();
    await expect(shop.getByTestId("dash-owner")).toBeVisible();
    await expect(shop.locator(".ticket")).toBeHidden();
    const t = await shop.getByTestId("owner-tools").evaluate((el) => el.getBoundingClientRect());
    const g = await shop.getByTestId("menu-grid").evaluate((el) => el.getBoundingClientRect());
    expect(t.top).toBeLessThan(g.top);

    const table = await browser.newPage();
    await guest(table, "Ling");
    await say(table, "milk tea");
    const no = await payAndNumber(table);
    const next = shop.getByTestId(`ticket-next-${no}`);
    await expect(next).toBeVisible();
    // Big enough for a thumb.
    const box = await next.boundingBox();
    expect(box.height).toBeGreaterThanOrEqual(40);
    await next.click();
    await expect(myOrder(table, no)).toContainText("Being made");
    await next.click();
    await expect(myOrder(table, no)).toContainText("On its way");
    await next.click();
    await expect(ticket(shop, no)).toHaveCount(0);
    await table.close();
  });
});
