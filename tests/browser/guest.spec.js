import { test, expect } from "@playwright/test";
import { guest, say, lastLine, dish, cartLine } from "./cafe.mjs";

test.describe("a guest orders", () => {
  test("tapping a dish fills the cart but spends nothing yet", async ({ page }) => {
    await guest(page);
    await dish(page, "latte").click();
    await expect(cartLine(page, "latte")).toContainText("Hot latte");
    await expect(cartLine(page, "latte")).toContainText("1×");
    await expect(page.getByTestId("cart-total")).toHaveText("4.8");
    await expect(page.getByTestId("balance")).toHaveText("50");
  });

  test("tapping the same dish twice stacks one line", async ({ page }) => {
    await guest(page);
    await dish(page, "egg_tart").click();
    await expect(cartLine(page, "egg_tart")).toContainText("1×");
    await dish(page, "egg_tart").click();
    await expect(cartLine(page, "egg_tart")).toContainText("2×");
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(1);
    await expect(page.getByTestId("cart-total")).toHaveText("4");
  });

  test("chat orders one dish by name", async ({ page }) => {
    await guest(page);
    await say(page, "latte");
    await expect(cartLine(page, "latte")).toContainText("Hot latte");
    await expect(page.getByTestId("cart-total")).toHaveText("4.8");
  });

  test("chat counts a quantity", async ({ page }) => {
    await guest(page);
    await say(page, "two pineapple buns");
    await expect(cartLine(page, "pineapple_bun")).toContainText("2×");
    await expect(page.getByTestId("cart-total")).toHaveText("4.8");
  });

  test("chat splits one sentence into several dishes", async ({ page }) => {
    await guest(page);
    await say(page, "two lattes and an egg tart");
    await expect(cartLine(page, "latte")).toContainText("2×");
    await expect(cartLine(page, "egg_tart")).toContainText("1×");
    // 4.80 × 2 + 2.00
    await expect(page.getByTestId("cart-total")).toHaveText("11.6");
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
    await expect(page.getByTestId("cart-total")).toHaveText("4.8");
    await say(page, "remove latte");
    await expect(cartLine(page, "latte")).toHaveCount(0);
    await expect(page.getByTestId("cart-total")).toHaveText("0");
  });

  test("clearing empties the cart", async ({ page }) => {
    await guest(page);
    await say(page, "latte and an egg tart");
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(2);
    await say(page, "clear");
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(0);
    await expect(page.getByTestId("cart-total")).toHaveText("0");
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
    await expect(page.getByTestId("cart-total")).toHaveText("4.8");
    await page.getByTestId("pay-usdc").click();
    await expect(page.getByTestId("paid-banner")).toContainText("Paid 4.8 USDC");
    await expect(page.getByTestId("balance")).toHaveText("45.2");
    await expect(page.getByTestId("cart-total")).toHaveText("0");
    await expect(cartLine(page, "latte")).toHaveCount(0);
  });

  test("paying by chat works like the button", async ({ page }) => {
    await guest(page);
    await say(page, "egg tart");
    await say(page, "pay");
    await expect(page.getByTestId("paid-banner")).toContainText("Paid 2 USDC");
    await expect(page.getByTestId("balance")).toHaveText("48");
  });

  test("an empty cart cannot be paid", async ({ page }) => {
    await guest(page);
    await page.getByTestId("pay-usdc").click();
    await expect(lastLine(page)).toContainText("empty");
    await expect(page.getByTestId("paid-banner")).toBeHidden();
    await expect(page.getByTestId("balance")).toHaveText("50");
  });

  test("a cart over the grant is refused with the shortfall", async ({ page }) => {
    await guest(page);
    await say(page, "10 french toasts"); // 5.40 × 10 = 54 against a 50 grant
    await expect(cartLine(page, "french_toast")).toContainText("10×");
    await page.getByTestId("pay-usdc").click();
    await expect(lastLine(page)).toContainText("need 54 USDC");
    await expect(page.getByTestId("balance")).toHaveText("50");
    await expect(page.getByTestId("paid-banner")).toBeHidden();
  });

  test("a guest is not shown the till", async ({ page }) => {
    await guest(page);
    await expect(page.getByTestId("owner-tools")).toBeHidden();
    await say(page, "payments");
    await expect(lastLine(page)).toContainText("owner");
    await expect(page.getByTestId("payment-row")).toHaveCount(0);
  });

  test("a guest cannot see the orders book", async ({ page }) => {
    await guest(page);
    await say(page, "orders");
    await expect(lastLine(page)).toContainText("owner");
  });

  test("nonsense gets a nudge, not a crash", async ({ page }) => {
    await guest(page);
    await say(page, "zxqw plover");
    await expect(lastLine(page)).toContainText("did not catch");
    await expect(page.getByTestId("quick-menu")).toBeVisible();
  });
});
