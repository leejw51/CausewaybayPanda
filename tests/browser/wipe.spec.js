/* The button on the door that wipes the shop back to a fresh install. It
   runs last in this project: everything the earlier specs left in the
   shared shop is what it clears. */
import { test, expect } from "@playwright/test";
import { guest, owner, dish, payAndNumber, say, uniqueDish } from "./cafe.mjs";

test.describe("clearing the shop", () => {
  test("the door wipes every order, guest, setting and dish, and everyone starts again", async ({
    page,
    browser,
  }) => {
    // Leave marks on the shop: an order, a renamed cafe, a new dish, a held key.
    const shop = await owner(page);
    const { name: dishName, id: dishId } = uniqueDish("wipe");
    await say(shop, `add item ${dishName} 30 dessert`);
    await expect(dish(shop, dishId)).toBeVisible();
    await shop.getByTestId("shop-setup").locator("summary").click();
    await shop.getByTestId("setup-name").fill("Doomed Corner");
    await shop.getByTestId("setup-save").click();
    await expect(shop.getByTestId("cafe-name-top")).toHaveText("Doomed Corner");
    await shop.getByTestId("ai-setup").locator("summary").click();
    await shop.getByTestId("ai-provider").selectOption("openrouter");
    await shop.getByTestId("ai-key").fill("or-doomed");
    await shop.getByTestId("ai-save").click();
    await expect(shop.getByTestId("ai-status")).toContainText("OpenRouter");

    const table = await browser.newPage();
    await guest(table, "Wipe Chan");
    await dish(table, "latte").click();
    const no = await payAndNumber(table);
    await expect(shop.getByTestId(`ticket-${no}`)).toBeVisible();
    await expect(shop.getByTestId("takings-total")).not.toHaveText("HK$0.00");

    // A third page at the door does the wiping. It confirms first.
    const door = await browser.newPage();
    await door.goto("/");
    await expect(door.getByTestId("cafe-name-door")).toHaveText("Doomed Corner");
    let asked = "";
    door.once("dialog", (d) => {
      asked = d.message();
      d.dismiss();
    });
    await door.getByTestId("clear-shop").click();
    expect(asked).toContain("Wipe everything?");
    // Dismissed: nothing happened.
    await door.waitForTimeout(300);
    await expect(door.getByTestId("cafe-name-door")).toHaveText("Doomed Corner");
    await expect(table.getByTestId("stage-app")).toBeVisible();

    door.once("dialog", (d) => d.accept());
    await door.getByTestId("clear-shop").click();

    // The door says so, and wears the shop's first name again.
    await expect(door.getByTestId("local-note")).toContainText("cleared");
    await expect(door.getByTestId("cafe-name-door")).toHaveText("Causewaybay Coffee");

    // The guest and the owner were sent back to the door, sessions forgotten.
    await expect(table.getByTestId("stage-door")).toBeVisible();
    await expect(table.getByTestId("stage-app")).toBeHidden();
    await expect(shop.getByTestId("stage-door")).toBeVisible();

    // Back in: a fresh shop.
    await shop.getByTestId("owner-pin").fill("panda");
    await shop.getByTestId("login-owner").click();
    await expect(shop.getByTestId("owner-tools")).toBeVisible();
    await expect(shop.getByTestId("takings-total")).toHaveText("HK$0.00");
    await expect(shop.getByTestId("queue-empty")).toBeVisible();
    await expect(shop.getByTestId("cafe-name-top")).toHaveText("Causewaybay Coffee");
    await expect(dish(shop, dishId)).toHaveCount(0);
    await expect(dish(shop, "latte")).toBeVisible();
    await expect(shop.getByTestId("ai-status")).toHaveText("Local parser only");

    await table.getByTestId("guest-name").fill("Wipe Chan");
    await table.getByTestId("login-guest").click();
    await expect(table.getByTestId("stage-app")).toBeVisible();
    await expect(table.getByTestId("history")).toBeHidden();
    await expect(table.getByTestId("my-orders")).toBeHidden();
    // Order numbers start from one again.
    await dish(table, "egg_tart").click();
    expect(await payAndNumber(table)).toBe(1);
    await table.close();
    await door.close();
  });
});
