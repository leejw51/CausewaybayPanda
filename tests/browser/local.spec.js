/* The whole cafe inside one browser tab. No socket is ever opened; the same
   Rust that runs the server runs here as WebAssembly over an in-memory
   store, and the page cannot tell the difference. */
import { test, expect } from "@playwright/test";
import { dish, cartLine, payAndNumber, myOrder, ticket, say, lastLine, GRANT } from "./cafe.mjs";

/** Open the tab-only cafe and note every socket the page tries to open. */
async function openLocal(page, path = "/?local") {
  const sockets = [];
  await page.addInitScript(() => {
    const Real = window.WebSocket;
    window.__sockets = [];
    const Wrapped = function (url, ...rest) {
      window.__sockets.push(String(url));
      return new Real(url, ...rest);
    };
    // The page reads WebSocket.OPEN and friends off the constructor.
    for (const k of ["CONNECTING", "OPEN", "CLOSING", "CLOSED"]) Wrapped[k] = Real[k];
    Wrapped.prototype = Real.prototype;
    window.WebSocket = Wrapped;
  });
  await page.goto(path);
  await expect(page.getByTestId("login-guest")).toBeVisible();
  sockets.read = () => page.evaluate(() => window.__sockets);
  return sockets;
}

async function guestLocal(page, name = "Mei") {
  const sockets = await openLocal(page);
  await page.getByTestId("guest-name").fill(name);
  await page.getByTestId("login-guest").click();
  await expect(page.getByTestId("stage-app")).toBeVisible();
  return sockets;
}


test.describe("a cafe with no server", () => {
  test("the door says this tab is the whole cafe, and opens no socket", async ({ page }) => {
    const sockets = await openLocal(page);
    await expect(page.getByTestId("local-note")).toContainText("This tab is the whole cafe");
    await expect(page.getByTestId("local-note")).toContainText("any pin opens the counter");
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await page.waitForTimeout(400);
    expect(await sockets.read()).toEqual([]);
  });

  test("a guest orders and pays entirely inside the tab", async ({ page }) => {
    await guestLocal(page);
    await expect(page.getByTestId("balance")).toHaveText(GRANT);
    await expect(page.getByTestId("mode-badge")).toContainText("Causewaybay Coin");
    await dish(page, "latte").click();
    await expect(cartLine(page, "latte")).toContainText("1×");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");
    const no = await payAndNumber(page);
    await expect(myOrder(page, no)).toContainText("Order received");
    await expect(page.getByTestId("balance")).toHaveText("HK$352.00");
  });

  test("chat is understood by the same parser", async ({ page }) => {
    await guestLocal(page);
    await say(page, "two pineapple buns and an egg tart");
    await expect(cartLine(page, "pineapple_bun")).toContainText("2×");
    await expect(cartLine(page, "egg_tart")).toContainText("1×");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$34.00");
    await say(page, "top up");
    await expect(page.getByTestId("balance")).toHaveText("HK$780.00");
  });

  test("the shop survives a reload from the tab's own storage", async ({ page }) => {
    await guestLocal(page, "Mei");
    await dish(page, "latte").click();
    const no = await payAndNumber(page);
    await page.reload();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await expect(page.getByTestId("role-label")).toHaveText("guest");
    await expect(myOrder(page, no)).toBeVisible();
    await expect(page.getByTestId("balance")).toHaveText("HK$352.00");
  });

  test("any pin opens the counter in a tab", async ({ page }) => {
    await openLocal(page);
    await page.getByTestId("owner-pin").fill("whatever");
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("owner-tools")).toBeVisible();
    await expect(page.getByTestId("takings-total")).toHaveText("HK$0.00");
  });

  test("the cafe runs itself when the owner throws the switch", async ({ page }) => {
    await openLocal(page, "/?local&tick=150");
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("owner-tools")).toBeVisible();
    await expect(page.getByTestId("queue-empty")).toBeVisible();

    const sw = page.getByTestId("auto-owner");
    await expect(sw).toBeVisible();
    await sw.click();
    await expect(sw).toHaveText("Stop the cafe");
    await expect(page.getByTestId("auto-dot")).toBeVisible();

    // Tickets arrive and money comes in with nobody else in the room.
    await expect(page.locator(".ticket-row").first()).toBeVisible({ timeout: 10_000 });
    await expect
      .poll(async () => Number((await page.getByTestId("takings-count").innerText()).match(/\d+/)[0]), {
        timeout: 15_000,
      })
      .toBeGreaterThanOrEqual(3);
    await expect(page.getByTestId("takings-total")).not.toHaveText("HK$0.00");

    await sw.click();
    await expect(sw).toHaveText("Run the cafe on its own");
    await expect(page.getByTestId("auto-dot")).toBeHidden();
  });

  test("a guest alone in the tab can let the kitchen run, and gets served", async ({ page }) => {
    // A slow enough beat that each state of the card can actually be seen
    // before the kitchen moves it on again.
    await openLocal(page, "/?local&tick=700");
    await page.getByTestId("guest-name").fill("Mei");
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();

    await dish(page, "egg_tart").click();
    const no = await payAndNumber(page);
    await expect(myOrder(page, no)).toContainText("Order received");

    // Nobody is at the counter, so the guest may set the cafe going.
    const sw = page.getByTestId("auto-guest");
    await expect(sw).toBeVisible();
    await sw.click();
    // The simulated kitchen works the queue oldest-first, so this order moves.
    await expect(myOrder(page, no)).toContainText(/Being made|Ready/, { timeout: 15_000 });
    await expect(myOrder(page, no)).toHaveCount(0, { timeout: 15_000 });
    await sw.click();
  });

  test("with a server present the page uses it, and the switch is the owner's alone", async ({ page }) => {
    // No ?local: the panda serving these files answers /health, so a socket opens.
    const sockets = await openLocal(page, "/");
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await expect.poll(() => sockets.read()).toHaveLength(1);
    await expect(page.getByTestId("auto-guest")).toBeHidden();
    await expect(page.getByTestId("local-note")).toBeHidden();
  });

  test("Leave returns to the door as nobody", async ({ page }) => {
    await guestLocal(page, "Mei");
    await dish(page, "latte").click();
    await page.getByTestId("leave").click();
    await expect(page.getByTestId("stage-door")).toBeVisible();
    // Nothing is remembered: a reload shows the door again, not the shop.
    await page.reload();
    await expect(page.getByTestId("stage-door")).toBeVisible();
    await expect(page.getByTestId("stage-app")).toBeHidden();
    // And the next guest in starts with a clean cart.
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(0);
  });

  test("the board can read in won", async ({ page }) => {
    await openLocal(page, "/?local&denom=KRW");
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await expect(page.getByTestId("mode-badge")).toContainText("KRW");
    await expect(dish(page, "latte")).toContainText("₩6,723");
    await dish(page, "latte").click();
    await expect(page.getByTestId("cart-total")).toHaveText("₩6,723");
  });

  test("the owner's choice of model is kept by the tab", async ({ page }) => {
    await openLocal(page);
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("owner-tools")).toBeVisible();
    await expect(page.getByTestId("ai-status")).toHaveText("Local parser only");
    await page.getByTestId("ai-setup").locator("summary").click();
    await page.getByTestId("ai-provider").selectOption("openrouter");
    await page.getByTestId("ai-key").fill("or-tab-key");
    await page.getByTestId("ai-save").click();
    await expect(page.getByTestId("ai-status")).toContainText("OpenRouter");

    // A reload rebuilds the engine from the tab's snapshot; the choice holds.
    await page.reload();
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("owner-tools")).toBeVisible();
    await expect(page.getByTestId("ai-status")).toContainText("OpenRouter");
  });

  test("a sentence the parser cannot read goes to the model, from the tab", async ({ page }) => {
    // The tab would call OpenRouter itself, with the tab's key. Stand in for
    // it at the network edge, so the whole path runs and nothing leaves.
    const asked = [];
    await page.route("https://openrouter.ai/**", async (route) => {
      const req = route.request();
      asked.push({ auth: req.headers()["authorization"], body: req.postDataJSON() });
      const text = String(req.postDataJSON().messages.slice(-1)[0].content).toLowerCase();
      const intent = text.includes("warm")
        ? { intent: "add", item_id: "latte", qty: 2 }
        : { intent: "help" };
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ choices: [{ message: { content: JSON.stringify(intent) } }] }),
      });
    });

    await openLocal(page);
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("owner-tools")).toBeVisible();
    await page.getByTestId("ai-setup").locator("summary").click();
    await page.getByTestId("ai-provider").selectOption("openrouter");
    await page.getByTestId("ai-key").fill("or-tab-key");
    await page.getByTestId("ai-save").click();
    await expect(page.getByTestId("ai-status")).toContainText("OpenRouter");

    // One person, both roles: back to the door, in as a guest.
    await page.getByTestId("leave").click();
    await page.getByTestId("guest-name").fill("Mei");
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();

    await say(page, "something warm to hold, please");
    await expect(page.getByTestId("transcript")).toContainText("Asking the model");
    await expect(cartLine(page, "latte")).toContainText("2×", { timeout: 10_000 });
    await expect(page.getByTestId("cart-total")).toHaveText("HK$76.00");

    // It went out with the tab's key and the shop's board in the prompt.
    expect(asked).toHaveLength(1);
    expect(asked[0].auth).toBe("Bearer or-tab-key");
    expect(JSON.stringify(asked[0].body)).toContain("latte");
    // A plain dish name never leaves: the parser answers it.
    await say(page, "egg tart");
    await expect(cartLine(page, "egg_tart")).toBeVisible();
    expect(asked).toHaveLength(1);
  });

  test("when the model is unreachable from the tab, the parser's answer stands", async ({ page }) => {
    await page.route("https://openrouter.ai/**", (route) => route.abort("failed"));
    await openLocal(page);
    await page.getByTestId("login-owner").click();
    await page.getByTestId("ai-setup").locator("summary").click();
    await page.getByTestId("ai-provider").selectOption("openrouter");
    await page.getByTestId("ai-key").fill("or-tab-key");
    await page.getByTestId("ai-save").click();
    await page.getByTestId("leave").click();
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await say(page, "something warm to hold, please");
    await expect(lastLine(page)).toContainText("did not catch", { timeout: 10_000 });
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(0);
  });
});
