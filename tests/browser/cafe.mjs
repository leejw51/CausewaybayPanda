/* Shared moves. Every spec drives the cafe through these, so a change to the
   door or the dock is one edit here rather than thirty. */
import { expect } from "@playwright/test";

export const GRANT = "50";

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
