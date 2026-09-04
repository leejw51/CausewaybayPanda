//! Causewaybay Coffee — the virtual cafe seeded on first boot.

use crate::money::MICRO;

pub struct SeedItem {
    pub id: &'static str,
    pub name: &'static str,
    pub name_zh: &'static str,
    pub description: &'static str,
    pub price_micro: i64,
    pub category: &'static str,
    pub image: &'static str,
}

pub const TREASURY: &str = "0xC0FFEE0000000000000000000000000000000c0f";

pub fn cafe_menu() -> &'static [SeedItem] {
    &[
        SeedItem {
            id: "latte",
            name: "Hot latte",
            name_zh: "熱鮮奶咖啡",
            description: "Double espresso, steamed milk, panda foam if the barista is in the mood.",
            price_micro: 480 * MICRO / 100,
            category: "coffee",
            image: "/assets/menu/latte.png",
        },
        SeedItem {
            id: "iced_latte",
            name: "Iced latte",
            name_zh: "凍鮮奶咖啡",
            description: "The same shot over ice. Harbour-window drink.",
            price_micro: 520 * MICRO / 100,
            category: "coffee",
            image: "/assets/menu/iced_latte.png",
        },
        SeedItem {
            id: "cappuccino",
            name: "Cappuccino",
            name_zh: "卡布奇諾",
            description: "Thick foam, cocoa dust, brass saucer.",
            price_micro: 480 * MICRO / 100,
            category: "coffee",
            image: "/assets/menu/cappuccino.png",
        },
        SeedItem {
            id: "yuenyeung",
            name: "Yuenyeung",
            name_zh: "鴛鴦",
            description: "Coffee married to silk-stocking tea. The house pour.",
            price_micro: 420 * MICRO / 100,
            category: "coffee",
            image: "/assets/menu/yuenyeung.png",
        },
        SeedItem {
            id: "milk_tea",
            name: "Silk milk tea",
            name_zh: "絲襪奶茶",
            description: "Pulled through a stocking, evaporated milk, no shortcuts.",
            price_micro: 360 * MICRO / 100,
            category: "tea",
            image: "/assets/menu/milk_tea.png",
        },
        SeedItem {
            id: "lemon_tea",
            name: "Iced lemon tea",
            name_zh: "凍檸茶",
            description: "Lemon wheels, crushed ice, the afternoon default.",
            price_micro: 320 * MICRO / 100,
            category: "tea",
            image: "/assets/menu/lemon_tea.png",
        },
        SeedItem {
            id: "pineapple_bun",
            name: "Pineapple bun",
            name_zh: "菠蘿油",
            description: "Crackly sugar crust, a cold butter slab if you say yes.",
            price_micro: 240 * MICRO / 100,
            category: "bakery",
            image: "/assets/menu/pineapple_bun.png",
        },
        SeedItem {
            id: "egg_tart",
            name: "Egg tart",
            name_zh: "蛋撻",
            description: "Flaky cup, still-warm custard. One is never one.",
            price_micro: 200 * MICRO / 100,
            category: "bakery",
            image: "/assets/menu/egg_tart.png",
        },
        SeedItem {
            id: "french_toast",
            name: "French toast",
            name_zh: "西多士",
            description: "Peanut butter, a brick of butter, golden syrup. Share it.",
            price_micro: 540 * MICRO / 100,
            category: "plates",
            image: "/assets/menu/french_toast.png",
        },
        SeedItem {
            id: "macaroni",
            name: "Macaroni soup",
            name_zh: "通粉湯",
            description: "Ham, a fried egg, the breakfast that is also lunch.",
            price_micro: 460 * MICRO / 100,
            category: "plates",
            image: "/assets/menu/macaroni.png",
        },
        SeedItem {
            id: "panda_bun",
            name: "Panda bun",
            name_zh: "熊貓包",
            description: "Steamed bun with a panda face. The signature. Soft.",
            price_micro: 280 * MICRO / 100,
            category: "bakery",
            image: "/assets/menu/panda_bun.png",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_cafe_is_priced_and_unique() {
        let menu = cafe_menu();
        assert!(menu.len() >= 8);
        let mut ids = std::collections::HashSet::new();
        for item in menu {
            assert!(item.price_micro > 0, "{}", item.id);
            assert!(ids.insert(item.id), "duplicate {}", item.id);
            assert!(!item.name.is_empty());
            assert!(!item.name_zh.is_empty());
        }
        assert!(menu.iter().any(|i| i.id == "latte"));
        assert!(menu.iter().any(|i| i.id == "panda_bun"));
        assert_eq!(TREASURY.len(), 42);
    }
}
