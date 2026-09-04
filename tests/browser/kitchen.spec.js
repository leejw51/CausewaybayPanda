/* The loop a food ordering system actually turns on: a guest pays, a ticket
   lands on the counter, the kitchen works it, and the guest watches it happen
   without touching anything. */
import { test, expect } from "@playwright/test";
import {
  guest,
  owner,
  say,
  lastLine,
  dish,
  cartLine,
  ticket,
  myOrder,
  payAndNumber,
  GRANT,
} from "./cafe.mjs";

test.describe("the counter and the table", () => {
  test("paying gives the guest a number to be called", async ({ page }) => {
    await guest(page);
    await expect(page.getByTestId("my-orders")).toBeHidden();
    await dish(page, "latte").click();
    const no = await payAndNumber(page);

    const card = myOrder(page, no);
    await expect(card).toBeVisible();
    await expect(card).toContainText(`#${no}`);
    await expect(card).toContainText("Order received");
    await expect(card).toContainText("1× Hot latte");
  });

  test("a ticket reaches the counter with no reload", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await guest(table, "Mei");
    await dish(table, "egg_tart").click();
    const no = await payAndNumber(table);

    const t = ticket(shop, no);
    await expect(t).toBeVisible();
    await expect(t).toContainText("Mei");
    await expect(t).toContainText("1× Egg tart");
    await expect(t).toContainText("HK$10.00");
    await table.close();
  });

  test("the kitchen walks an order to the guest, one step at a time", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await guest(table, "Ling");
    await dish(table, "milk_tea").click();
    const no = await payAndNumber(table);
    await expect(myOrder(table, no)).toContainText("Order received");

    await shop.getByTestId(`ticket-next-${no}`).click();
    await expect(myOrder(table, no)).toContainText("Being made");

    await shop.getByTestId(`ticket-next-${no}`).click();
    await expect(myOrder(table, no)).toContainText("Ready");

    // Handed over: it leaves both the queue and the guest's card.
    await shop.getByTestId(`ticket-next-${no}`).click();
    await expect(ticket(shop, no)).toHaveCount(0);
    await expect(myOrder(table, no)).toHaveCount(0);
    await table.close();
  });

  test("a cancelled order leaves the queue and the guest's card", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await guest(table, "Kwok");
    await dish(table, "panda_bun").click();
    const no = await payAndNumber(table);
    await expect(myOrder(table, no)).toBeVisible();

    await shop.getByTestId(`ticket-cancel-${no}`).click();
    await expect(ticket(shop, no)).toHaveCount(0);
    await expect(myOrder(table, no)).toHaveCount(0);
    await table.close();
  });

  test("one table never sees another table's order", async ({ browser }) => {
    const a = await browser.newPage();
    const b = await browser.newPage();
    await guest(a, "Ah Fai");
    await guest(b, "Ah Ming");

    await dish(a, "latte").click();
    const no = await payAndNumber(a);
    await expect(myOrder(a, no)).toBeVisible();

    // B is in the same room and hears nothing about it.
    await expect(b.getByTestId("my-orders")).toBeHidden();
    await expect(myOrder(b, no)).toHaveCount(0);
    await a.close();
    await b.close();
  });

  test("numbers count up across guests", async ({ page, browser }) => {
    const shop = await owner(page);
    const given = [];
    for (const who of ["First", "Second"]) {
      const table = await browser.newPage();
      await guest(table, who);
      await dish(table, "egg_tart").click();
      given.push(await payAndNumber(table));
      await table.close();
    }
    expect(given[1]).toBe(given[0] + 1);
    await expect(ticket(shop, given[0])).toContainText("First");
    await expect(ticket(shop, given[1])).toContainText("Second");
  });

  test("an order a guest is waiting on survives a reload", async ({ page, browser }) => {
    await guest(page, "Mei");
    await dish(page, "latte").click();
    const no = await payAndNumber(page);
    await expect(myOrder(page, no)).toBeVisible();

    // A phone reloads. The browser remembers who it was and walks straight
    // back in — same name, same order card, no door.
    await page.reload();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await expect(page.getByTestId("stage-door")).toBeHidden();
    await expect(page.getByTestId("role-label")).toHaveText("guest");
    await expect(myOrder(page, no)).toBeVisible();
    await expect(myOrder(page, no)).toContainText("1× Hot latte");

    // And the counter, opened fresh, still has the ticket.
    const shop = await browser.newPage();
    await owner(shop);
    await expect(ticket(shop, no)).toBeVisible();
    await expect(ticket(shop, no)).toContainText("Mei");
    await shop.close();
  });
});

test.describe("the cart is a cart", () => {
  test("plus and minus change a line without retyping it", async ({ page }) => {
    await guest(page);
    await dish(page, "latte").click();
    await expect(cartLine(page, "latte")).toContainText("1×");

    await page.getByTestId("more-latte").click();
    await expect(cartLine(page, "latte")).toContainText("2×");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$76.00");

    await page.getByTestId("less-latte").click();
    await expect(cartLine(page, "latte")).toContainText("1×");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");

    // Minus at one takes the line off entirely.
    await page.getByTestId("less-latte").click();
    await expect(cartLine(page, "latte")).toHaveCount(0);
    await expect(page.getByTestId("cart-empty")).toBeVisible();
  });

  test("an empty cart cannot be paid", async ({ page }) => {
    await guest(page);
    await expect(page.getByTestId("cart-empty")).toBeVisible();
    await expect(page.getByTestId("pay-usdc")).toBeDisabled();
    await dish(page, "latte").click();
    await expect(page.getByTestId("pay-usdc")).toBeEnabled();
  });
});

test.describe("test money", () => {
  test("the faucet tops a guest up", async ({ page }) => {
    await guest(page);
    await expect(page.getByTestId("balance")).toHaveText(GRANT);
    await page.getByTestId("faucet").click();
    await expect(page.getByTestId("balance")).toHaveText("HK$780.00");
    await expect(lastLine(page)).toContainText("Topped up");
  });

  test("the faucet stops at a ceiling", async ({ page }) => {
    await guest(page);
    // 390 → 780 → 1170 → capped at 1560.
    for (let i = 0; i < 3; i++) {
      await page.getByTestId("faucet").click();
      await page.waitForTimeout(120);
    }
    await expect(page.getByTestId("balance")).toHaveText("HK$1,560.00");
    await expect(page.getByTestId("faucet")).toBeDisabled();
  });

  test("a guest who runs short is told to top up", async ({ page }) => {
    await guest(page);
    await say(page, "10 french toasts"); // HK$420 against a HK$390 purse
    await expect(cartLine(page, "french_toast")).toContainText("10×");
    await page.getByTestId("pay-usdc").click();
    await expect(lastLine(page)).toContainText("you need HK$420.00");
    await expect(lastLine(page)).toContainText("Top up");

    // And topping up lets the order through.
    await page.getByTestId("faucet").click();
    await expect(page.getByTestId("balance")).toHaveText("HK$780.00");
    await page.getByTestId("pay-usdc").click();
    await expect(page.getByTestId("paid-banner")).toContainText("Paid HK$420.00");
    await expect(page.getByTestId("balance")).toHaveText("HK$360.00");
  });

  test("asking for the faucet in chat works too", async ({ page }) => {
    await guest(page);
    await say(page, "top up");
    await expect(page.getByTestId("balance")).toHaveText("HK$780.00");
  });

  test("simulation shows no chain hash to follow", async ({ page }) => {
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-usdc").click();
    await expect(page.getByTestId("paid-banner")).toContainText("Paid HK$38.00");
    // Nothing happened on a chain, so there is nothing to link to.
    await expect(page.getByTestId("paid-link")).toHaveCount(0);
    await expect(page.getByTestId("paid-banner")).not.toContainText("demo-");
  });

  test("a simulation shop refuses a wallet payment and says why", async ({ page }) => {
    await guest(page);
    await expect(page.getByTestId("pay-wallet")).toBeHidden();
    await dish(page, "latte").click();
    await say(page, "pay with wallet");
    await expect(lastLine(page)).toContainText("wallet payment is off");
    await expect(page.getByTestId("paid-banner")).toBeHidden();
  });
});
