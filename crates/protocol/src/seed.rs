//! Causewaybay Coffee — the virtual cafe seeded on first boot.

use crate::denom::Denom;

pub struct SeedItem {
    pub id: &'static str,
    pub name: &'static str,
    pub name_zh: &'static str,
    pub description: &'static str,
    /// What the board says, in whole Hong Kong dollars — this is a Causeway
    /// Bay cafe. Stored as micro-USDC; the peg makes the conversion stable.
    pub price_hkd: i64,
    pub category: &'static str,
    pub image: &'static str,
}

impl SeedItem {
    /// The settlement figure behind the board price.
    pub fn price_micro(&self) -> i64 {
        Denom::preset("HKD")
            .expect("HKD is a preset")
            .micro_from_minor(self.price_hkd as i128 * 100)
    }
}

pub const TREASURY: &str = "0xC0FFEE0000000000000000000000000000000c0f";

pub fn cafe_menu() -> &'static [SeedItem] {
    &[
        SeedItem {
            id: "latte",
            name: "Hot latte",
            name_zh: "熱鮮奶咖啡",
            description: "Double espresso, steamed milk, panda foam if the barista is in the mood.",
            price_hkd: 38,
            category: "coffee",
            image: "/assets/menu/latte.png",
        },
        SeedItem {
            id: "iced_latte",
            name: "Iced latte",
            name_zh: "凍鮮奶咖啡",
            description: "The same shot over ice. Harbour-window drink.",
            price_hkd: 42,
            category: "coffee",
            image: "/assets/menu/iced_latte.png",
        },
        SeedItem {
            id: "cappuccino",
            name: "Cappuccino",
            name_zh: "卡布奇諾",
            description: "Thick foam, cocoa dust, brass saucer.",
            price_hkd: 38,
            category: "coffee",
            image: "/assets/menu/cappuccino.png",
        },
        SeedItem {
            id: "yuenyeung",
            name: "Yuenyeung",
            name_zh: "鴛鴦",
            description: "Coffee married to silk-stocking tea. The house pour.",
            price_hkd: 32,
            category: "coffee",
            image: "/assets/menu/yuenyeung.png",
        },
        SeedItem {
            id: "milk_tea",
            name: "Silk milk tea",
            name_zh: "絲襪奶茶",
            description: "Pulled through a stocking, evaporated milk, no shortcuts.",
            price_hkd: 28,
            category: "tea",
            image: "/assets/menu/milk_tea.png",
        },
        SeedItem {
            id: "lemon_tea",
            name: "Iced lemon tea",
            name_zh: "凍檸茶",
            description: "Lemon wheels, crushed ice, the afternoon default.",
            price_hkd: 26,
            category: "tea",
            image: "/assets/menu/lemon_tea.png",
        },
        SeedItem {
            id: "pineapple_bun",
            name: "Pineapple bun",
            name_zh: "菠蘿油",
            description: "Crackly sugar crust, a cold butter slab if you say yes.",
            price_hkd: 12,
            category: "bakery",
            image: "/assets/menu/pineapple_bun.png",
        },
        SeedItem {
            id: "egg_tart",
            name: "Egg tart",
            name_zh: "蛋撻",
            description: "Flaky cup, still-warm custard. One is never one.",
            price_hkd: 10,
            category: "bakery",
            image: "/assets/menu/egg_tart.png",
        },
        SeedItem {
            id: "french_toast",
            name: "French toast",
            name_zh: "西多士",
            description: "Peanut butter, a brick of butter, golden syrup. Share it.",
            price_hkd: 42,
            category: "plates",
            image: "/assets/menu/french_toast.png",
        },
        SeedItem {
            id: "macaroni",
            name: "Macaroni soup",
            name_zh: "通粉湯",
            description: "Ham, a fried egg, the breakfast that is also lunch.",
            price_hkd: 36,
            category: "plates",
            image: "/assets/menu/macaroni.png",
        },
        SeedItem {
            id: "panda_bun",
            name: "Panda bun",
            name_zh: "熊貓包",
            description: "Steamed bun with a panda face. The signature. Soft.",
            price_hkd: 22,
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
            assert!(item.price_micro() > 0, "{}", item.id);
            assert!(ids.insert(item.id), "duplicate {}", item.id);
            assert!(!item.name.is_empty());
            assert!(!item.name_zh.is_empty());
        }
        assert!(menu.iter().any(|i| i.id == "latte"));
        assert!(menu.iter().any(|i| i.id == "panda_bun"));
        assert_eq!(TREASURY.len(), 42);
    }
}
