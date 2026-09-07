#!/usr/bin/env bash
# Paint Causewaybay Coffee with Grok. Skip files that already exist unless --force.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/static/assets"
ZKP="/Volumes/nvidia/vivid/CausewaybayZkp/love2d/assets"
STYLE="Illustrated in the same 16-bit Causeway Bay night as a Wonder Boy candy palette: saturated neon magenta, harbour cyan, brass gold, cream lantern light, painterly but sharp, no photoreal people, no watermark, no text, no letters, no logo, no UI."

mkdir -p "$OUT/menu" "$OUT/zkp"
KEY="${XAI_API_KEY:-${GROK_API_KEY:-}}"
FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force|-f) FORCE=1 ;;
  esac
done

if [ -z "$KEY" ]; then
  echo "set XAI_API_KEY or GROK_API_KEY" >&2
  exit 1
fi

# Reuse the ZKP street as the cafe window — that is the graphical world.
if [ -f "$ZKP/title_bg.png" ]; then
  cp "$ZKP/title_bg.png" "$OUT/zkp/street.png"
  echo "  copied zkp street"
fi
if [ -f "$ZKP/title_bg_p.png" ]; then
  cp "$ZKP/title_bg_p.png" "$OUT/zkp/street_portrait.png"
fi
if [ -f "$ZKP/ui_panel.png" ]; then
  cp "$ZKP/ui_panel.png" "$OUT/zkp/panel.png"
fi
if [ -f "$ZKP/ui_coin.png" ]; then
  cp "$ZKP/ui_coin.png" "$OUT/zkp/coin.png"
fi
if [ -f "$ZKP/icon.png" ]; then
  cp "$ZKP/icon.png" "$OUT/zkp/icon.png"
fi

paint() {
  local file="$1" aspect="$2" prompt="$3"
  if [ -f "$file" ] && [ "$FORCE" != "1" ]; then
    echo "  have  $(basename "$file")"
    return
  fi
  echo "  paint $(basename "$file") …"
  "$ROOT/tools/grok_image.sh" "$file" "$aspect" "$prompt $STYLE"
}

echo "painting Causewaybay Coffee into $OUT"

paint "$OUT/panda.png" 1:1 \
"A round original panda barista character, three-quarter view, cream and black fur, wearing a jade-tile mosaic apron and a tiny brass name badge, holding a steel milk pitcher, friendly half-lidded eyes, solid flat magenta background for sprite knockout."

paint "$OUT/icon.png" 1:1 \
"App icon: a round panda face in a jade apron, brass circle rim, neon magenta night glow, simple and readable at small size, cream fur, no text."

paint "$OUT/cafe_interior.png" 16:9 \
"Interior of a small Hong Kong cha chaan teng coffee shop at night looking out through a wide window onto neon Causeway Bay. Jade mosaic tiles, brass rails, steam, a wooden booth in the foreground, hanging warm lamps, empty of people, cinematic wide shot."

paint "$OUT/hero.png" 16:9 \
"Causeway Bay Hong Kong at dusk from a cafe doorway: green tram, red lanterns, neon CAFE-like glow without readable letters, palm trees, harbour and towers, candy 16-bit painterly city."

paint "$OUT/menu/latte.png" 1:1 \
"A ceramic cup of hot latte with a panda face in the foam, on a brass cafe counter, neon magenta window light, product shot, solid dark wood background."

paint "$OUT/menu/iced_latte.png" 1:1 \
"A tall glass of iced latte with cream layers and ice, brass straw, Hong Kong cafe counter at night, product shot, no text."

paint "$OUT/menu/cappuccino.png" 1:1 \
"A cappuccino in a thick ceramic cup with thick foam and cocoa dust, jade tile counter, warm lamp, product shot, no text."

paint "$OUT/menu/yuenyeung.png" 1:1 \
"A Hong Kong yuenyeung coffee-and-tea drink in a speckled cha chaan teng glass, condensed milk swirl, brass saucer, night cafe, product shot, no text."

paint "$OUT/menu/milk_tea.png" 1:1 \
"Silk-stocking Hong Kong milk tea in a traditional tall glass with a metal holder, rich amber colour, steam, jade tiles, product shot, no text."

paint "$OUT/menu/lemon_tea.png" 1:1 \
"Iced lemon tea in a tall glass with lemon wheels and crushed ice, condensation, neon cyan rim light, Hong Kong cafe, product shot, no text."

paint "$OUT/menu/pineapple_bun.png" 1:1 \
"A Hong Kong pineapple bun (bolo bao) split with a thick slab of cold butter melting inside, on a small plate, brass counter, product shot, no text."

paint "$OUT/menu/egg_tart.png" 1:1 \
"A glossy Hong Kong egg tart in a flaky golden pastry cup, one tart close up, warm lamp, wood plate, product shot, no text."

paint "$OUT/menu/french_toast.png" 1:1 \
"Hong Kong cafe French toast: thick fried bread, peanut butter, a slab of butter, golden syrup, on an oval plate, cha chaan teng night, product shot, no text."

paint "$OUT/menu/macaroni.png" 1:1 \
"Hong Kong breakfast macaroni soup in a speckled bowl with ham and a fried egg, spoon, jade mosaic table, product shot, no text."

paint "$OUT/menu/panda_bun.png" 1:1 \
"A steamed bun printed like a cute panda face, black sesame eyes, on a bamboo steamer, brass and steam, night cafe, product shot, no text."

# The guest's journey: one plate per stage of an order, and the moment it
# lands. Painted on flat magenta so the page can knock the background out,
# the way the mascot is.
mkdir -p "$OUT/journey"
SPRITE="Single centred object, chunky readable silhouette, dark outline, nothing else in frame, no shadow on the ground, nothing magenta or pink on the object itself, solid flat magenta background for sprite knockout."

paint "$OUT/journey/received.png" 1:1 \
"A small paper order ticket with a brass clip and a stamped panda paw, slightly curled, cafe ticket icon. $SPRITE"

paint "$OUT/journey/making.png" 1:1 \
"A steaming brass milk pitcher beside a brass portafilter, wisps of cream steam rising, barista at work icon, only these two objects. $SPRITE"

paint "$OUT/journey/ready.png" 1:1 \
"A round polished brass counter bell with a cyan glow around it, order ready icon. $SPRITE"

paint "$OUT/journey/delivered.png" 1:1 \
"A round cream-and-black panda barista in a jade apron holding out a wooden tray with a latte and an egg tart, seen from the front, handing over icon. $SPRITE"


echo "done."
