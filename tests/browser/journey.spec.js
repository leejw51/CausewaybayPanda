/* The guest's journey: the four plates under an order, the bar that creeps
   between the kitchen's real words, the burst when the sign comes on, and
   the tray landing when it is handed over. */
import { test, expect } from "@playwright/test";
import { guest, owner, dish, payAndNumber, myOrder, ticket } from "./cafe.mjs";

function step(card, stage) {
  return card.locator(`.journey-step[data-stage="${stage}"]`);
}

test.describe("the guest's journey", () => {
  test("a paid order shows four plates with the first lit, and the bar creeping", async ({ page }) => {
    await guest(page, "Journey Chan");
    await dish(page, "latte").click();
    const no = await payAndNumber(page);
    const card = myOrder(page, no);
    await expect(card.getByTestId("journey")).toHaveAttribute("data-stage", "placed");
    await expect(card.locator(".journey-step")).toHaveCount(4);
    await expect(step(card, "placed")).toHaveClass(/active/);
    await expect(step(card, "preparing")).not.toHaveClass(/active|done/);
    // The plates are Grok's, knocked out of their magenta and inlined.
    await expect(step(card, "placed").locator("img")).toHaveAttribute("src", /^data:image\/png/);
    // Nothing has happened yet, but the bar is moving: an estimate, and it says so.
    await expect(card.getByTestId("journey-fill")).toHaveClass(/creeping/);
    await expect(card.getByTestId("journey-note")).toContainText("estimate");
    const w1 = await card.getByTestId("journey-fill").evaluate((el) => el.getBoundingClientRect().width);
    await page.waitForTimeout(700);
    const w2 = await card.getByTestId("journey-fill").evaluate((el) => el.getBoundingClientRect().width);
    expect(w2).toBeGreaterThan(w1);
  });

  test("the guest's card says where the latest order is, with its plate", async ({ page }) => {
    await guest(page, "Now Chan");
    await dish(page, "egg_tart").click();
    const no = await payAndNumber(page);
    const now = page.getByTestId("gdash-now");
    await expect(now).toBeVisible();
    await expect(now).toHaveAttribute("data-stage", "placed");
    await expect(page.getByTestId("gdash-now-headline")).toHaveText(`#${no} · Received`);
    await expect(page.getByTestId("gdash-now-line")).toHaveText("1× Egg tart");
    await expect(now.locator("img")).toHaveAttribute("src", /^data:image\/png/);
  });

  test("the kitchen's words move the plates, the sign bursts, and the tray lands", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await guest(table, "Burst Chan");
    await dish(table, "milk_tea").click();
    const no = await payAndNumber(table);
    const card = myOrder(table, no);

    await shop.getByTestId(`ticket-next-${no}`).click();
    await expect(card.getByTestId("journey")).toHaveAttribute("data-stage", "preparing");
    await expect(step(card, "placed")).toHaveClass(/done/);
    await expect(step(card, "preparing")).toHaveClass(/active/);
    await expect(card.getByTestId("journey-note")).toContainText("On the bar");
    await expect(table.getByTestId("gdash-now-headline")).toHaveText(`#${no} · Being made`);

    await shop.getByTestId(`ticket-next-${no}`).click();
    await expect(card.getByTestId("journey")).toHaveAttribute("data-stage", "ready");
    await expect(step(card, "ready")).toHaveClass(/active/);
    // Particles over the card, and gone again once they have fallen.
    await expect(card.getByTestId("burst")).toBeVisible();
    await expect(card.getByTestId("burst")).toHaveCount(0, { timeout: 5_000 });
    await expect(card.getByTestId("journey-fill")).not.toHaveClass(/creeping/);
    await expect(table.getByTestId("gdash-now")).toHaveAttribute("data-stage", "ready");

    // Served: the tray lands, a bigger shower, and the card leaves
    // a moment later rather than vanishing under the guest's eyes.
    await shop.getByTestId(`ticket-next-${no}`).click();
    await expect(ticket(shop, no)).toHaveCount(0);
    await expect(card).toContainText("Served");
    await expect(step(card, "collected")).toHaveClass(/active/);
    await expect(card.getByTestId("burst")).toBeVisible();
    await expect(table.getByTestId("gdash-now-headline")).toHaveText(`#${no} · Served`);
    await expect(card).toHaveCount(0, { timeout: 6_000 });
    await expect(table.getByTestId("gdash-now")).toHaveAttribute("data-stage", "none");
    await expect(table.getByTestId("gdash-now-headline")).toHaveText("Nothing on the way");
    await table.close();
  });

  test("with two orders on the way the card counts them", async ({ page }) => {
    await guest(page, "Two Chan");
    await dish(page, "latte").click();
    await payAndNumber(page);
    await dish(page, "egg_tart").click();
    const second = await payAndNumber(page);
    await expect(page.getByTestId("gdash-now-headline")).toHaveText(`#${second} · Received`);
    await expect(page.getByTestId("gdash-now-line")).toContainText("2 orders on the way");
  });

  test("the bar picks up where it was after a reload", async ({ page }) => {
    await guest(page, "Reload Chan");
    await dish(page, "latte").click();
    const no = await payAndNumber(page);
    await page.reload();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    const card = myOrder(page, no);
    await expect(card.getByTestId("journey")).toHaveAttribute("data-stage", "placed");
    await expect(card.getByTestId("journey-fill")).toHaveClass(/creeping/);
    await expect(page.getByTestId("gdash-now-headline")).toHaveText(`#${no} · Received`);
  });

  test("a person who asked for stillness gets no creep and no particles", async ({ page, browser }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    const shop = await owner(page);
    const table = await browser.newPage();
    await table.emulateMedia({ reducedMotion: "reduce" });
    await guest(table, "Still Chan");
    await dish(table, "egg_tart").click();
    const no = await payAndNumber(table);
    const card = myOrder(table, no);
    await expect(card.getByTestId("journey-fill")).not.toHaveClass(/creeping/);
    await shop.getByTestId(`ticket-next-${no}`).click();
    await shop.getByTestId(`ticket-next-${no}`).click();
    await expect(step(card, "ready")).toHaveClass(/active/);
    await table.waitForTimeout(300);
    await expect(card.getByTestId("burst")).toHaveCount(0);
    await shop.getByTestId(`ticket-next-${no}`).click();
    await expect(card).toHaveCount(0, { timeout: 6_000 });
    await table.close();
  });
});

test.describe("the guest's book", () => {
  test("every order a guest placed is in their book, newest first, with its detail", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await guest(table, "Book Chan");
    await expect(table.getByTestId("history")).toBeHidden();

    await dish(table, "latte").click();
    await dish(table, "latte").click();
    await dish(table, "egg_tart").click();
    const first = await payAndNumber(table);
    await expect(table.getByTestId("history")).toBeVisible();
    await expect(table.getByTestId("history-count")).toHaveText("1 order");
    await table.getByTestId("history").locator("summary").click();

    await dish(table, "milk_tea").click();
    const second = await payAndNumber(table);
    await expect(table.getByTestId("history-count")).toHaveText("2 orders");
    const rows = table.getByTestId("history-list").locator(".history-row");
    await expect(rows).toHaveCount(2);
    await expect(rows.first()).toHaveAttribute("data-testid", `history-${second}`);

    // Open the first order: lines with prices, the total, and its journey.
    await table.getByTestId(`history-${first}`).click();
    const detail = table.getByTestId(`history-detail-${first}`);
    await expect(detail).toBeVisible();
    await expect(detail.locator(".history-lines li")).toHaveCount(2);
    await expect(detail.locator(".history-lines li").first()).toContainText("2× Hot latte");
    await expect(detail.locator(".history-lines li").first()).toContainText("HK$76.00");
    await expect(detail.locator(".history-lines li").first()).toContainText("HK$38.00 each");
    await expect(detail.getByTestId("history-total")).toHaveText("HK$86.00");
    await expect(detail.getByTestId("journey")).toHaveAttribute("data-stage", "placed");
    await expect(detail).toContainText("Placed today");

    // The book follows the kitchen, and a row left open stays open.
    await shop.getByTestId(`ticket-next-${first}`).click();
    await shop.getByTestId(`ticket-next-${first}`).click();
    await shop.getByTestId(`ticket-next-${first}`).click();
    const row = table.getByTestId(`history-${first}`);
    await expect(row).toHaveClass(/collected/);
    await expect(row.getByTestId("history-status")).toHaveText("Served");
    await expect(table.getByTestId(`history-detail-${first}`).getByTestId("journey")).toHaveAttribute(
      "data-stage",
      "collected"
    );
    // Served orders stay in the book after they leave the card.
    await expect(myOrder(table, first)).toHaveCount(0, { timeout: 6_000 });
    await expect(row).toBeVisible();

    // A cancelled order is in the book too, marked so.
    await shop.getByTestId(`ticket-cancel-${second}`).click();
    await expect(table.getByTestId(`history-${second}`)).toHaveClass(/cancelled/);
    await expect(table.getByTestId(`history-${second}`).getByTestId("history-status")).toHaveText("Cancelled");

    // Tap the row again to fold the detail away; the detail itself is
    // content, and a tap on it (to select a line) leaves the row open.
    await table.getByTestId(`history-detail-${first}`).click();
    await expect(table.getByTestId(`history-detail-${first}`)).toHaveCount(1);
    await row.locator(".history-no").click();
    await expect(table.getByTestId(`history-detail-${first}`)).toHaveCount(0);
    await table.close();
  });

  test("the book survives a reload", async ({ page }) => {
    await guest(page, "Reload Book");
    await dish(page, "egg_tart").click();
    const no = await payAndNumber(page);
    await page.reload();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await page.getByTestId("history").locator("summary").click();
    await expect(page.getByTestId(`history-${no}`)).toBeVisible();
    await page.getByTestId(`history-${no}`).click();
    await expect(page.getByTestId(`history-detail-${no}`)).toContainText("1× Egg tart");
  });

  test("the owner has no guest book", async ({ page }) => {
    await owner(page);
    await expect(page.getByTestId("history")).toBeHidden();
  });
});
