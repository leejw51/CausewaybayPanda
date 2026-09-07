/* Shared moves. Every spec drives the cafe through these, so a change to the
   door or the dock is one edit here rather than thirty. */
import { expect } from "@playwright/test";

/** The simulation purse, as the default HKD board reads it. */
export const GRANT = "HK$390.00";
/** Seed prices, in the money a guest actually sees. */
export const PRICE = {
  latte: "HK$38.00",
  iced_latte: "HK$42.00",
  cappuccino: "HK$38.00",
  yuenyeung: "HK$32.00",
  milk_tea: "HK$28.00",
  lemon_tea: "HK$26.00",
  pineapple_bun: "HK$12.00",
  egg_tart: "HK$10.00",
  french_toast: "HK$42.00",
  macaroni: "HK$36.00",
  panda_bun: "HK$22.00",
};

/** Open the door and walk in as a guest. */
export async function guest(page, name = "Mei") {
  await page.goto("/");
  await page.getByTestId("guest-name").fill(name);
  await page.getByTestId("login-guest").click();
  await expect(page.getByTestId("stage-app")).toBeVisible();
  await expect(page.getByTestId("role-label")).toHaveText("guest");
  return page;
}

/** Open the door and walk in as the owner. */
export async function owner(page, pin = "panda") {
  await page.goto("/");
  await page.getByTestId("owner-pin").fill(pin);
  await page.getByTestId("login-owner").click();
  await expect(page.getByTestId("stage-app")).toBeVisible();
  await expect(page.getByTestId("role-label")).toHaveText("owner");
  return page;
}

/** Say something in the dock. A cart or menu change answers with no chat line,
    so callers assert on the outcome they expect and let it retry. */
export async function say(page, text) {
  await page.getByTestId("chat-input").fill(text);
  await page.getByTestId("chat-send").click();
  await expect(page.getByTestId("chat-input")).toHaveValue("");
}

/** The last line the panda said. */
export function lastLine(page) {
  return page.getByTestId("transcript").locator("p").last();
}

/** Pay, then read back the number the counter gave you. Order numbers count
    up across the whole run, so no test may assume it is first. */
export async function payAndNumber(page) {
  const banner = page.getByTestId("paid-banner");
  // A second pay in a row must not read the first order's banner.
  const before = (await banner.innerText().catch(() => "")).match(/order #(\d+)/)?.[1] || "";
  await page.getByTestId("pay-usdc").click();
  await expect
    .poll(async () => (await banner.innerText().catch(() => "")).match(/order #(\d+)/)?.[1] || "")
    .not.toBe(before);
  const text = await banner.innerText();
  return Number(text.match(/order #(\d+)/)[1]);
}

/** The counter's ticket for order number `no`. */
export function ticket(page, no) {
  return page.getByTestId(`ticket-${no}`);
}

/** The card a guest watches for order number `no`. */
export function myOrder(page, no) {
  return page.getByTestId(`my-order-${no}`);
}

/** A dish tile on the board. */
export function dish(page, id) {
  return page.getByTestId(`dish-${id}`);
}

/** A line in the cart. */
export function cartLine(page, id) {
  return page.getByTestId(`cart-${id}`);
}

/** A name no other test will collide with. */
export function uniqueDish(prefix) {
  const n = Math.random().toString(36).slice(2, 8);
  return { name: `${prefix} ${n}`, id: `${prefix}_${n}`.toLowerCase() };
}
