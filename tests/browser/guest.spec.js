import { test, expect } from "@playwright/test";
import { guest, say, lastLine, dish, cartLine, payAndNumber } from "./cafe.mjs";

test.describe("a guest orders", () => {
  test("tapping a dish fills the cart but spends nothing yet", async ({ page }) => {
    await guest(page);
    await dish(page, "latte").click();
    await expect(cartLine(page, "latte")).toContainText("Hot latte");
    await expect(cartLine(page, "latte")).toContainText("1×");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");
    await expect(page.getByTestId("balance")).toHaveText("HK$390.00");
  });

  test("tapping the same dish twice stacks one line", async ({ page }) => {
    await guest(page);
    await dish(page, "egg_tart").click();
    await expect(cartLine(page, "egg_tart")).toContainText("1×");
    await dish(page, "egg_tart").click();
    await expect(cartLine(page, "egg_tart")).toContainText("2×");
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(1);
    await expect(page.getByTestId("cart-total")).toHaveText("HK$20.00");
  });

  test("chat orders one dish by name", async ({ page }) => {
    await guest(page);
    await say(page, "latte");
    await expect(cartLine(page, "latte")).toContainText("Hot latte");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");
  });

  test("chat counts a quantity", async ({ page }) => {
    await guest(page);
    await say(page, "two pineapple buns");
    await expect(cartLine(page, "pineapple_bun")).toContainText("2×");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$24.00");
  });

  test("chat splits one sentence into several dishes", async ({ page }) => {
    await guest(page);
    await say(page, "two lattes and an egg tart");
    await expect(cartLine(page, "latte")).toContainText("2×");
    await expect(cartLine(page, "egg_tart")).toContainText("1×");
    // HK$38 × 2 + HK$10
    await expect(page.getByTestId("cart-total")).toHaveText("HK$86.00");
  });

  test("a nickname finds the dish", async ({ page }) => {
    await guest(page);
    await say(page, "bolo bao");
    await expect(cartLine(page, "pineapple_bun")).toBeVisible();
  });

  // "remove latte" names a dish, and the matcher used to read it as an order —
  // asking to remove a latte added one instead.
  test("removing by chat removes, and never adds", async ({ page }) => {
    await guest(page);
    await say(page, "two lattes");
    await expect(cartLine(page, "latte")).toContainText("2×");
    await say(page, "remove latte");
    await expect(cartLine(page, "latte")).toContainText("1×");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");
    await say(page, "remove latte");
    await expect(cartLine(page, "latte")).toHaveCount(0);
    await expect(page.getByTestId("cart-total")).toHaveText("HK$0.00");
  });

  test("clearing empties the cart", async ({ page }) => {
    await guest(page);
    await say(page, "latte and an egg tart");
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(2);
    await say(page, "clear");
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(0);
    await expect(page.getByTestId("cart-total")).toHaveText("HK$0.00");
  });

  test("the quick buttons order the same way the board does", async ({ page }) => {
    await guest(page);
    await page.getByTestId("quick-menu").click();
    const add = page.getByTestId("quick-add").first();
    await expect(add).toBeVisible();
    const label = await add.textContent();
    await add.click();
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(1);
    // The button carries "Hot latte · 4.8"; the cart must show that dish.
    await expect(page.getByTestId("cart-lines")).toContainText(label.split(" ·")[0]);
  });

  test("paying moves the money and clears the cart", async ({ page }) => {
    await guest(page);
    await dish(page, "latte").click();
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");
    await page.getByTestId("pay-usdc").click();
    await expect(page.getByTestId("paid-banner")).toContainText("Paid HK$38.00");
    await expect(page.getByTestId("balance")).toHaveText("HK$352.00");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$0.00");
    await expect(cartLine(page, "latte")).toHaveCount(0);
  });

  test("paying by chat works like the button", async ({ page }) => {
    await guest(page);
    await say(page, "egg tart");
    await say(page, "pay");
    await expect(page.getByTestId("paid-banner")).toContainText("Paid HK$10.00");
    await expect(page.getByTestId("balance")).toHaveText("HK$380.00");
  });

  test("an empty cart cannot be paid", async ({ page }) => {
    await guest(page);
    // The button is shut rather than answering with an error.
    await expect(page.getByTestId("pay-usdc")).toBeDisabled();
    await expect(page.getByTestId("paid-banner")).toBeHidden();
    await expect(page.getByTestId("balance")).toHaveText("HK$390.00");
  });

  test("a cart over the grant is refused with the shortfall", async ({ page }) => {
    await guest(page);
    await say(page, "10 french toasts"); // HK$42 × 10 against a HK$390 purse
    await expect(cartLine(page, "french_toast")).toContainText("10×");
    await page.getByTestId("pay-usdc").click();
    await expect(lastLine(page)).toContainText("you need HK$420.00");
    await expect(page.getByTestId("balance")).toHaveText("HK$390.00");
    await expect(page.getByTestId("paid-banner")).toBeHidden();
  });

  test("a guest is not shown the till", async ({ page }) => {
    await guest(page);
    await expect(page.getByTestId("owner-tools")).toBeHidden();
    await say(page, "payments");
    await expect(lastLine(page)).toContainText("owner");
    await expect(page.getByTestId("payment-row")).toHaveCount(0);
  });

  test("a guest asking for orders is shown their own book, not the shop's", async ({ page }) => {
    await guest(page, `Book ${Math.random().toString(36).slice(2, 6)}`);
    await say(page, "latte");
    const no = await payAndNumber(page);
    await say(page, "orders");
    await expect(page.getByTestId("history")).toBeVisible();
    await expect(page.getByTestId("history")).toHaveJSProperty("open", true);
    await expect(page.getByTestId(`history-${no}`)).toBeVisible();
    // Only theirs: the shop's other guests are not in it.
    await expect(page.getByTestId("history-list").locator(".history-row")).toHaveCount(1);
    await expect(page.getByTestId("order-row")).toHaveCount(0);
  });

  test("nonsense gets a nudge, not a crash", async ({ page }) => {
    await guest(page);
    await say(page, "zxqw plover");
    await expect(lastLine(page)).toContainText("did not catch");
    await expect(page.getByTestId("quick-menu")).toBeVisible();
  });

  // A first visit has nothing to read back; after the first order the card
  // above the board says what they have had here and what is on the way.
  test("the guest's card appears with the first order and keeps a running tally", async ({ page }) => {
    await guest(page, `Tally ${Math.random().toString(36).slice(2, 6)}`);
    await expect(page.getByTestId("dash-guest")).toBeHidden();
    await say(page, "two lattes and an egg tart");
    await payAndNumber(page);
    const card = page.getByTestId("dash-guest");
    await expect(card).toBeVisible();
    await expect(page.getByTestId("gdash-orders")).toHaveText("1");
    await expect(page.getByTestId("gdash-spent")).toHaveText("HK$86.00");
    await expect(page.getByTestId("gdash-favourite")).toHaveText("Hot latte");
    await expect(page.getByTestId("gdash-favourite-note")).toContainText("2 so far");
    await expect(page.getByTestId("gdash-open")).toHaveText("1");

    await dish(page, "egg_tart").click();
    await dish(page, "egg_tart").click();
    await dish(page, "egg_tart").click();
    await payAndNumber(page);
    await expect(page.getByTestId("gdash-orders")).toHaveText("2");
    await expect(page.getByTestId("gdash-spent")).toHaveText("HK$116.00");
    // Four tarts now beat two lattes.
    await expect(page.getByTestId("gdash-favourite")).toHaveText("Egg tart");
    await expect(page.getByTestId("gdash-open")).toHaveText("2");
  });

  test("a guest can ask for their card in words", async ({ page }) => {
    await guest(page, `Words ${Math.random().toString(36).slice(2, 6)}`);
    await say(page, "latte");
    await payAndNumber(page);
    await page.reload();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await say(page, "my visits");
    await expect(page.getByTestId("dash-guest")).toBeVisible();
    await expect(page.getByTestId("gdash-orders")).toHaveText("1");
  });
});
