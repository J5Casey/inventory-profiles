//! Inventory Profiles' engine: settings, sort ranks and the sort's click
//! plan, tool break protection and auto-refill decisions. It knows nothing of
//! Horizon; `lib.rs` binds it to the game through the mod API, and
//! `engine/Cargo.toml` builds this file on its own so `cargo test` runs
//! without the SDK.

pub const ID: &str = "inventory_profiles";

pub const KEY_SORT: &str = "sort";
pub const KEY_SORT_DEFAULT: &str = "key.keyboard.r";
pub const KEY_QUICK_STACK: &str = "quick_stack";
pub const KEY_QUICK_STACK_DEFAULT: &str = "key.keyboard.g";

/// Colours carry alpha: the mod API draws text with alpha 0 as nothing.
pub const BACKGROUND: u32 = 0x7800_0000;
pub const COLOR_INFO: u32 = 0xFF55_FF55;
pub const COLOR_WARN: u32 = 0xFFFF_AA00;
pub const COLOR_ALERT: u32 = 0xFFFF_5555;

/// How long a notice stays, in seconds.
pub const TOAST_SECONDS: f32 = 2.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    /// 0 = Category (Creative Tab), 1 = Alphabetical (A-Z), 2 = Stack Count (High-Low), 3 = Item ID
    pub sort_mode: u8,
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
            auto_refill: true,
            tool_protection: 1,
            quick_stack: true,
            notifications: true,
        }
    }
}

/// A setting on the mod's page in Options > Mods: its key and what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Toggle(&'static str, bool),
    /// The option keys (the last part of their translation keys) and the
    /// default's index.
    Choice(&'static str, &'static [&'static str], usize),
}

/// The page's settings, in order. Labels are `inventory_profiles.setting.<key>`,
/// a choice's options `inventory_profiles.setting.<key>.<option>`.
pub const SETTINGS: [Setting; 6] = [
    Setting::Toggle("enabled", true),
    Setting::Choice("sort_mode", &["category", "name", "count", "id"], 0),
    Setting::Toggle("auto_refill", true),
    Setting::Choice("tool_protection", &["off", "two", "five", "ten"], 1),
    Setting::Toggle("quick_stack", true),
    Setting::Toggle("notifications", true),
];

impl Settings {
    /// The settings as the game keeps them: `toggle` and `choice` read one
    /// by key (None is its default).
    pub fn read(
        toggle: impl Fn(&str) -> Option<bool>,
        choice: impl Fn(&str) -> Option<u32>,
    ) -> Self {
        let d = Self::default();
        let pick = |key: &str, default: u8| choice(key).map_or(default, |v| (v % 4) as u8);
        Self {
            enabled: toggle("enabled").unwrap_or(d.enabled),
            sort_mode: pick("sort_mode", d.sort_mode),
            auto_refill: toggle("auto_refill").unwrap_or(d.auto_refill),
            tool_protection: pick("tool_protection", d.tool_protection),
            quick_stack: toggle("quick_stack").unwrap_or(d.quick_stack),
            notifications: toggle("notifications").unwrap_or(d.notifications),
        }
    }

    pub fn sort_mode_name(mode: u8) -> &'static str {
        match mode % 4 {
            0 => "Category (Creative)",
            1 => "Alphabetical (A-Z)",
            2 => "Stack Count (High-Low)",
            _ => "Item ID",
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
}

#[derive(Clone, Debug, Default)]
pub struct HandSnapshot {
    pub valid: bool,
    pub slot: usize,
    pub item: String,
    pub count: u32,
}

/// The notice the HUD shows for a moment after the mod did something.
#[derive(Clone, Debug, Default)]
pub struct Toast {
    pub text: String,
    pub color: u32,
    pub timer: f32,
}

impl Toast {
    pub fn push(&mut self, settings: &Settings, text: impl Into<String>, color: u32) {
        if settings.notifications {
            self.text = text.into();
            self.color = color;
            self.timer = TOAST_SECONDS;
        }
    }
    pub fn tick(&mut self, dt: f32) {
        if self.timer > 0.0 {
            self.timer = (self.timer - dt).max(0.0);
        }
    }
    pub fn showing(&self) -> bool {
        self.timer > 0.0 && !self.text.is_empty()
    }
}

/// A stack's place in a sort: its rank in the order, then its name.
pub type Rank = (usize, String);

/// The rank tuple a sort orders stacks by.
pub fn compute_sort_rank(
    sort_mode: u8,
    creative_order: usize,
    display_name: &str,
    item_id: &str,
    count: u32,
) -> Rank {
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

/// A stack as the sort sees it: its kind (stacks of a kind stack together)
/// and its count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub kind: usize,
    pub count: u32,
}

/// A hand and slots under `AbstractContainerMenu.doClick`'s left `PICKUP`.
struct Sim<'a> {
    slots: Vec<Option<Piece>>,
    hand: Option<Piece>,
    max: &'a [u32],
    clicks: Vec<usize>,
}

impl Sim<'_> {
    fn click(&mut self, i: usize) {
        self.clicks.push(i);
        let slot = self.slots[i].take();
        match (self.hand.take(), slot) {
            // Take the stack, or put the hand's down.
            (None, s) => self.hand = s,
            (Some(h), None) => self.slots[i] = Some(h),
            // The same kind: as much as fits joins the slot.
            (Some(mut h), Some(mut s)) if s.kind == h.kind => {
                let n = self.max[s.kind].saturating_sub(s.count).min(h.count);
                s.count += n;
                h.count -= n;
                self.slots[i] = Some(s);
                self.hand = (h.count > 0).then_some(h);
            }
            // Another kind: they swap.
            (Some(h), Some(s)) => {
                self.slots[i] = Some(h);
                self.hand = Some(s);
            }
        }
    }
    fn partial(&self, i: usize) -> Option<Piece> {
        self.slots[i].filter(|s| s.count < self.max[s.kind])
    }
}

/// The indices of `slots` to left-click, in order, to sort them (Horizon's
/// sorting extra's plan, `inv_sort::plan`, which a mod can't call): partial
/// stacks of a kind merge first, then the stacks go in rank order, full
/// stacks before the partial one, empty slots last. `max` and `rank` are by
/// kind. Empty when they are sorted already, or when the clicks couldn't
/// reach the order (stacks over their maximum).
pub fn plan(slots: &[Option<Piece>], rank: &dyn Fn(Piece) -> Rank, max: &[u32]) -> Vec<usize> {
    let n = slots.len();
    if slots.iter().flatten().any(|p| max.get(p.kind).is_none()) {
        return Vec::new();
    }
    let mut sim = Sim {
        slots: slots.to_vec(),
        hand: None,
        max,
        clicks: Vec::new(),
    };
    // Merge: each later partial stack of the kind tops up the first one;
    // what doesn't fit goes back.
    for i in 0..n {
        for j in i + 1..n {
            let Some(s) = sim.partial(i) else {
                break;
            };
            if !sim.partial(j).is_some_and(|t| t.kind == s.kind) {
                continue;
            }
            sim.click(j);
            sim.click(i);
            if sim.hand.is_some() {
                sim.click(j);
            }
        }
    }
    let stacks: Vec<Piece> = sim.slots.iter().flatten().copied().collect();
    let ranks: Vec<Rank> = stacks.iter().map(|&p| rank(p)).collect();
    let mut order: Vec<usize> = (0..stacks.len()).collect();
    order.sort_by(|&a, &b| {
        ranks[a]
            .cmp(&ranks[b])
            .then(stacks[b].count.cmp(&stacks[a].count))
    });
    let target: Vec<Option<Piece>> = order
        .iter()
        .map(|&i| Some(stacks[i]))
        .chain(std::iter::repeat(None))
        .take(n)
        .collect();
    // Order: each slot takes its stack from further on; what it held goes
    // where that stack was.
    for (i, want) in target.iter().enumerate() {
        if sim.slots[i] == *want {
            continue;
        }
        let Some(j) = (i + 1..n).find(|&j| sim.slots[j] == *want) else {
            return Vec::new();
        };
        sim.click(j);
        sim.click(i);
        if sim.hand.is_some() {
            sim.click(j);
        }
    }
    if sim.hand.is_some() || sim.slots != target {
        return Vec::new();
    }
    sim.clicks
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
    fn settings_read_with_defaults() {
        let s = Settings::read(|_| None, |_| None);
        assert_eq!(s, Settings::default());
        assert!(s.enabled);
        assert_eq!(s.sort_mode, 0);
        assert_eq!(s.tool_protection, 1);
        let s = Settings::read(
            |k| (k == "quick_stack").then_some(false),
            |k| match k {
                "sort_mode" => Some(1),
                "tool_protection" => Some(3),
                _ => None,
            },
        );
        assert!(!s.quick_stack && s.auto_refill);
        assert_eq!((s.sort_mode, s.tool_protection), (1, 3));
        for setting in SETTINGS {
            if let Setting::Choice(_, options, default) = setting {
                assert!(default < options.len());
            }
        }
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

    /// Plays `clicks` on `slots` as the menu would.
    fn play(slots: &[Option<Piece>], max: &[u32], clicks: &[usize]) -> Vec<Option<Piece>> {
        let mut sim = Sim {
            slots: slots.to_vec(),
            hand: None,
            max,
            clicks: Vec::new(),
        };
        for &i in clicks {
            sim.click(i);
        }
        assert!(sim.hand.is_none(), "the hand ends empty");
        sim.slots
    }

    #[test]
    fn the_plan_merges_then_orders() {
        let p = |kind, count| Some(Piece { kind, count });
        // kind 0 ranks after kind 1; kind 0's two partial stacks merge.
        let slots = [p(0, 10), None, p(1, 3), p(0, 60)];
        let max = [64, 64];
        let rank = |piece: Piece| (1 - piece.kind, String::new());
        let clicks = plan(&slots, &rank, &max);
        assert!(!clicks.is_empty());
        let end = play(&slots, &max, &clicks);
        assert_eq!(end, vec![p(1, 3), p(0, 64), p(0, 6), None]);
        // Sorted already: no click.
        assert!(plan(&end, &rank, &max).is_empty());
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
