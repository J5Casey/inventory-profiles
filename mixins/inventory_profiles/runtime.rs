// Inventory Profiles runtime state & pure logic.
// Compiles both standalone (`cargo test`) and inside `horizon-client` via `horizon-mixins`.

use std::sync::Mutex;

pub const ID: &str = "inventory_profiles";
pub const NAME: &str = "Inventory Profiles";
pub const VERSION: &str = "1.0.2";
pub const DESCRIPTION: &str = "1:1 Inventory Profiles Next & Inventory Sorter: R-key & UI sorting (Category, A-Z, Count, ID), Auto-Refill depleted stacks, Tool Break Protection, and G-key Quick-Stack to chests.";

pub const KEY_SORT: &str = "sort";
pub const KEY_SORT_LABEL: &str = "Sort Inventory / Container";
pub const KEY_SORT_DEFAULT: &str = "key.keyboard.r";

pub const KEY_QUICK_STACK: &str = "quick_stack";
pub const KEY_QUICK_STACK_LABEL: &str = "Quick-Stack to Container";
pub const KEY_QUICK_STACK_DEFAULT: &str = "key.keyboard.g";

pub const BACKGROUND: u32 = 0x7800_0000;
pub const COLOR_INFO: u32 = 0x0055_FF55;
pub const COLOR_WARN: u32 = 0x00FF_AA00;
pub const COLOR_ALERT: u32 = 0x00FF_5555;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    /// 0 = Category (Creative Tab), 1 = Alphabetical (A-Z), 2 = Stack Count (High-Low), 3 = Item ID
    pub sort_mode: u8,
    pub sort_buttons: bool,
    pub auto_refill: bool,
    /// 0 = Off, 1 = <= 2 durability, 2 = <= 5 durability, 3 = <= 10 durability
    pub tool_protection: u8,
    pub quick_stack: bool,
    pub notifications: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            sort_mode: 0,
            sort_buttons: true,
            auto_refill: true,
            tool_protection: 1,
            quick_stack: true,
            notifications: true,
        }
    }
}

impl Settings {
    pub fn load<'a>(mut get: impl FnMut(&str) -> Option<String> + 'a) -> Self {
        let mut s = Self::default();
        if let Some(v) = get("enabled") {
            s.enabled = v != "false" && v != "0";
        }
        if let Some(v) = get("sort_mode").and_then(|v| v.parse::<u8>().ok()) {
            s.sort_mode = v % 4;
        }
        if let Some(v) = get("sort_buttons") {
            s.sort_buttons = v != "false" && v != "0";
        }
        if let Some(v) = get("auto_refill") {
            s.auto_refill = v != "false" && v != "0";
        }
        if let Some(v) = get("tool_protection").and_then(|v| v.parse::<u8>().ok()) {
            s.tool_protection = v % 4;
        }
        if let Some(v) = get("quick_stack") {
            s.quick_stack = v != "false" && v != "0";
        }
        if let Some(v) = get("notifications") {
            s.notifications = v != "false" && v != "0";
        }
        s
    }

    pub fn stored(&self) -> [(&'static str, String); 7] {
        [
            ("enabled", if self.enabled { "true" } else { "false" }.into()),
            ("sort_mode", self.sort_mode.to_string()),
            ("sort_buttons", if self.sort_buttons { "true" } else { "false" }.into()),
            ("auto_refill", if self.auto_refill { "true" } else { "false" }.into()),
            ("tool_protection", self.tool_protection.to_string()),
            ("quick_stack", if self.quick_stack { "true" } else { "false" }.into()),
            ("notifications", if self.notifications { "true" } else { "false" }.into()),
        ]
    }

    pub fn sort_mode_name(mode: u8) -> &'static str {
        match mode % 4 {
            0 => "Category (Creative)",
            1 => "Alphabetical (A-Z)",
            2 => "Stack Count (High-Low)",
            _ => "Item ID",
        }
    }

    pub fn tool_protection_name(mode: u8) -> &'static str {
        match mode % 4 {
            0 => "Off",
            1 => "<= 2 Durability",
            2 => "<= 5 Durability",
            _ => "<= 10 Durability",
        }
    }

    pub fn tool_protection_threshold(mode: u8) -> u32 {
        match mode % 4 {
            0 => 0,
            1 => 2,
            2 => 5,
            _ => 10,
        }
    }

    pub fn labels(&self) -> Vec<String> {
        let on_off = |b: bool| if b { "ON" } else { "OFF" };
        vec![
            format!("Inventory Profiles: {}", on_off(self.enabled)),
            format!("Sort Order: {}", Self::sort_mode_name(self.sort_mode)),
            format!("UI Sort Buttons: {}", on_off(self.sort_buttons)),
            format!("Auto-Refill Hand Stack: {}", on_off(self.auto_refill)),
            format!("Tool Break Protection: {}", Self::tool_protection_name(self.tool_protection)),
            format!("Quick-Stack to Chest (G): {}", on_off(self.quick_stack)),
            format!("HUD Action Notifications: {}", on_off(self.notifications)),
        ]
    }

    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.enabled = !self.enabled,
            1 => self.sort_mode = (self.sort_mode + 1) % 4,
            2 => self.sort_buttons = !self.sort_buttons,
            3 => self.auto_refill = !self.auto_refill,
            4 => self.tool_protection = (self.tool_protection + 1) % 4,
            5 => self.quick_stack = !self.quick_stack,
            6 => self.notifications = !self.notifications,
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct HandSnapshot {
    pub valid: bool,
    pub slot: usize,
    pub item: String,
    pub count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeState {
    pub loaded: bool,
    pub settings: Settings,
    pub prev_hand: HandSnapshot,
    pub toast_text: String,
    pub toast_color: u32,
    pub toast_timer: f32,
}

static STATE: Mutex<RuntimeState> = Mutex::new(RuntimeState {
    loaded: false,
    settings: Settings {
        enabled: true,
        sort_mode: 0,
        sort_buttons: true,
        auto_refill: true,
        tool_protection: 1,
        quick_stack: true,
        notifications: true,
    },
    prev_hand: HandSnapshot {
        valid: false,
        slot: 0,
        item: String::new(),
        count: 0,
    },
    toast_text: String::new(),
    toast_color: COLOR_INFO,
    toast_timer: 0.0,
});

pub fn settings<'a>(get: impl FnMut(&str) -> Option<String> + 'a) -> Settings {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if !guard.loaded {
        guard.settings = Settings::load(get);
        guard.loaded = true;
    }
    guard.settings
}

pub fn set_settings(s: Settings) {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    guard.settings = s;
    guard.loaded = true;
}

pub fn with_state<R>(f: impl FnOnce(&mut RuntimeState) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

pub fn push_toast(text: impl Into<String>, color: u32) {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.settings.notifications {
        guard.toast_text = text.into();
        guard.toast_color = color;
        guard.toast_timer = 2.4;
    }
}

/// Computes the `(usize, String)` rank tuple expected by `horizon_client::inv_sort::plan`.
pub fn compute_sort_rank(
    sort_mode: u8,
    creative_order: usize,
    display_name: &str,
    item_id: &str,
    count: u32,
) -> (usize, String) {
    let lower_name = display_name.to_ascii_lowercase();
    let short_id = item_id.strip_prefix("minecraft:").unwrap_or(item_id);
    match sort_mode % 4 {
        // 0: Creative Tab Category, then display name
        0 => (creative_order, lower_name),
        // 1: Alphabetical (A-Z) by display name, tie-broken by item ID
        1 => (0, format!("{lower_name}\0{short_id}")),
        // 2: Stack Count (High-Low), tie-broken by display name
        2 => ((u32::MAX - count) as usize, lower_name),
        // 3: Item ID (A-Z)
        _ => (0, short_id.to_ascii_lowercase()),
    }
}

/// Returns true if an item with `damage` and `max_damage` is dangerously close to breaking
/// according to `tool_protection` mode (`1` => <=2, `2` => <=5, `3` => <=10).
pub fn should_protect_tool(damage: u32, max_damage: u32, tool_protection: u8) -> bool {
    let threshold = Settings::tool_protection_threshold(tool_protection);
    if threshold == 0 || max_damage <= threshold || damage == 0 {
        return false;
    }
    let remaining = max_damage.saturating_sub(damage);
    remaining > 0 && remaining <= threshold
}

/// Checks if the active hotbar slot just became empty after holding an item on the same slot.
/// Returns `Some(previous_item_id)` if auto-refill should search the inventory for a replacement.
pub fn check_auto_refill_needed(
    prev: &HandSnapshot,
    current_slot: usize,
    current_item: Option<&str>,
) -> Option<String> {
    if !prev.valid || prev.slot != current_slot || prev.item.is_empty() || prev.count == 0 {
        return None;
    }
    if current_item.is_none() {
        Some(prev.item.clone())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_and_clicks() {
        let mut s = Settings::default();
        assert!(s.enabled);
        assert_eq!(s.sort_mode, 0);
        assert_eq!(s.tool_protection, 1);

        s.click(1);
        assert_eq!(s.sort_mode, 1);
        s.click(4);
        assert_eq!(s.tool_protection, 2);

        let map: std::collections::HashMap<_, _> = s.stored().into_iter().collect();
        let loaded = Settings::load(|k| map.get(k).cloned());
        assert_eq!(s, loaded);
    }

    #[test]
    fn sort_ranks_order_correctly_across_all_modes() {
        // Category mode
        let r_stone = compute_sort_rank(0, 10, "Stone", "minecraft:stone", 64);
        let r_apple = compute_sort_rank(0, 250, "Apple", "minecraft:apple", 16);
        assert!(r_stone < r_apple);

        // Alphabetical mode (Apple before Stone despite higher creative order)
        let a_stone = compute_sort_rank(1, 10, "Stone", "minecraft:stone", 64);
        let a_apple = compute_sort_rank(1, 250, "Apple", "minecraft:apple", 16);
        assert!(a_apple < a_stone);

        // Count mode (64 before 16)
        let c_64 = compute_sort_rank(2, 250, "Apple", "minecraft:apple", 64);
        let c_16 = compute_sort_rank(2, 10, "Stone", "minecraft:stone", 16);
        assert!(c_64 < c_16);

        // ID mode
        let id_acacia = compute_sort_rank(3, 50, "Planks", "minecraft:acacia_planks", 64);
        let id_oak = compute_sort_rank(3, 10, "Planks", "minecraft:oak_planks", 64);
        assert!(id_acacia < id_oak);
    }

    #[test]
    fn tool_protection_thresholds_work() {
        // Diamond pickaxe: max_damage = 1561
        assert!(!should_protect_tool(0, 1561, 1));
        assert!(!should_protect_tool(1550, 1561, 1)); // 11 left
        assert!(should_protect_tool(1559, 1561, 1)); // 2 left (<= 2)
        assert!(should_protect_tool(1560, 1561, 1)); // 1 left (<= 2)
        assert!(!should_protect_tool(1560, 1561, 0)); // Off
        assert!(should_protect_tool(1552, 1561, 3)); // 9 left (<= 10)
    }

    #[test]
    fn auto_refill_detects_depleted_slot() {
        let prev = HandSnapshot {
            valid: true,
            slot: 2,
            item: "minecraft:cobblestone".into(),
            count: 1,
        };
        assert_eq!(
            check_auto_refill_needed(&prev, 2, None),
            Some("minecraft:cobblestone".into())
        );
        // Changed hotbar slot -> do not refill
        assert_eq!(check_auto_refill_needed(&prev, 3, None), None);
        // Still has item -> do not refill
        assert_eq!(
            check_auto_refill_needed(&prev, 2, Some("minecraft:cobblestone")),
            None
        );
    }
}
