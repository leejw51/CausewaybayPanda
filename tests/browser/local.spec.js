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

/** The tab's own owner pin, as the door announces it. */
async function ownerPin(page) {
  const note = await page.getByTestId("local-note").innerText();
  return note.match(/pin is (\d{4})/)[1];
}

test.describe("a cafe with no server", () => {
  test("the door says this tab is the whole cafe, and opens no socket", async ({ page }) => {
    const sockets = await openLocal(page);
    await expect(page.getByTestId("local-note")).toContainText("This tab is the whole cafe");
    await expect(page.getByTestId("local-note")).toContainText(/pin is \d{4}/);
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

  test("the owner pin minted for this tab opens the counter", async ({ page }) => {
    await openLocal(page);
    const pin = await ownerPin(page);
    await page.getByTestId("owner-pin").fill(pin);
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("owner-tools")).toBeVisible();
    await expect(page.getByTestId("takings-total")).toHaveText("HK$0.00");
    // A wrong pin is still a wrong pin.
    await page.reload();
    await page.getByTestId("owner-pin").fill("0000");
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("door-error")).toContainText("pin");
  });

  test("the cafe runs itself when the owner throws the switch", async ({ page }) => {
    await openLocal(page, "/?local&tick=150");
    const pin = await ownerPin(page);
    await page.getByTestId("owner-pin").fill(pin);
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
    await openLocal(page, "/?local&tick=150");
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
});
