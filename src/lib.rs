//! Inventory Profiles for Horizon: sorting, auto-refill, tool break
//! protection and quick-stack, as a native mod on the mod API
//! (`horizon-mod-api`, lib `mod_api`). The decisions are in `engine.rs`;
//! this file reads the player's menu and clicks it as a hand would
//! (`MenuRef::click`, ordinary `ContainerClick`s a server accepts).
#![forbid(unsafe_code)]

pub mod engine;

use engine::{HandSnapshot, Piece, Setting, Settings, Toast};
use mod_api::{
    ClientHost, Hooks, InputKeyEvent, ItemKey, ItemStack, KeyAction, KeyConflictContext,
    KeyRegistration, MenuClick, MenuRef, MouseButton, Registrar, RegistrationContext,
    RenderFramePreEvent, RenderGuiPostEvent, ScreenCharTypedPreEvent, ScreenRenderPostEvent,
    SettingSpec, SwapWith, Tag,
};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use engine::ID;

/// An open menu screen: its menu and which of its slots are the open
/// container's storage and the player's main inventory (menu order).
#[derive(Clone, Debug)]
struct Open {
    menu: MenuRef,
    storage: Vec<usize>,
    main: Vec<usize>,
}

#[derive(Default)]
struct State {
    prev_hand: HandSnapshot,
    toast: Toast,
    open: Option<Open>,
    /// The last key's character went into a text box (a recipe book's
    /// search): its key is typing, not a command.
    typed_in_field: bool,
    order: HashMap<ItemKey, usize>,
    names: HashMap<ItemKey, String>,
}

#[derive(Default)]
struct InventoryProfiles {
    state: Mutex<State>,
}

impl InventoryProfiles {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn settings(host: &dyn ClientHost) -> Settings {
    Settings::read(|k| host.setting_bool(k), |k| host.setting_choice(k))
}

/// The storage slots of menu type `menu` (None: the player's inventory)
/// with `slots` slots: a chest's, barrel's, shulker box's, ender chest's,
/// hopper's, dispenser's or dropper's, a mount's chest. Empty for any other
/// menu (the crafting table, workstations, a content mod's menu type).
fn storage(menu: Option<&str>, slots: usize) -> Vec<usize> {
    let own = slots.saturating_sub(36);
    match menu {
        Some(m)
            if m.starts_with("minecraft:generic_9x")
                || m == "minecraft:shulker_box"
                || m == "minecraft:hopper"
                || m == "minecraft:generic_3x3" =>
        {
            (0..own).collect()
        }
        Some(m) if m.starts_with("horizon:mount") && own > 2 => (2..own).collect(),
        _ => Vec::new(),
    }
}

/// The player's main inventory's 27 slots in menu `menu` with `slots`
/// slots: the inventory screen's 9..36, else the 27 before the hotbar, which
/// every menu puts last.
fn main_slots(menu: Option<&str>, slots: usize) -> Vec<usize> {
    match menu {
        None => (9..36).collect(),
        Some(_) if slots >= 36 => (slots - 36..slots - 9).collect(),
        Some(_) => Vec::new(),
    }
}

fn component<'a>(stack: &'a ItemStack, id: &str) -> Option<&'a Tag> {
    stack
        .components()?
        .as_nbt()
        .iter()
        .find(|(k, _)| k.to_string_lossy() == id)
        .map(|(_, v)| v)
}

fn int_component(stack: &ItemStack, id: &str) -> Option<u32> {
    match component(stack, id)? {
        Tag::Int(n) => u32::try_from(*n).ok(),
        _ => None,
    }
}

fn max_of(host: &dyn ClientHost, stack: &ItemStack) -> u32 {
    int_component(stack, "minecraft:max_stack_size")
        .or_else(|| host.max_stack_size(stack.item()))
        .unwrap_or(64)
        .max(1)
}

fn max_damage(host: &dyn ClientHost, stack: &ItemStack) -> u32 {
    int_component(stack, "minecraft:max_damage")
        .or_else(|| host.max_damage(stack.item()))
        .unwrap_or(0)
}

/// The name an item shows (its translation; an anvil name is not read).
fn name(host: &dyn ClientHost, st: &mut State, item: ItemKey) -> String {
    st.names
        .entry(item)
        .or_insert_with(|| {
            let id = item.id(host);
            let (ns, path) = id.split_once(':').unwrap_or(("minecraft", &id));
            host.translate(&format!("item.{ns}.{path}"))
                .or_else(|| host.translate(&format!("block.{ns}.{path}")))
                .unwrap_or_else(|| path.to_owned())
        })
        .clone()
}

/// An item's place in the item registry (the creative tabs' order is not
/// readable by a mod).
fn order(host: &dyn ClientHost, st: &mut State, item: ItemKey) -> usize {
    if st.order.is_empty() {
        st.order = host
            .item_keys()
            .into_iter()
            .enumerate()
            .map(|(i, k)| (k, i))
            .collect();
    }
    st.order.get(&item).copied().unwrap_or(usize::MAX)
}

fn slot(host: &dyn ClientHost, menu: MenuRef, index: usize) -> Option<ItemStack> {
    menu.slot(host, index).flatten()
}

impl InventoryProfiles {
    /// The open menu screen, while it is the player's current menu.
    fn open(&self, host: &dyn ClientHost) -> Option<Open> {
        let st = self.state();
        let open = st.open.as_ref()?;
        let now = host.local_player()?.container_menu(host)?;
        (now == open.menu && open.menu.slot_boxes(host).is_some()).then(|| open.clone())
    }

    /// Sorts `slots` (menu order) of the open menu with left clicks.
    fn sort_slots(&self, host: &mut dyn ClientHost, s: &Settings, menu: MenuRef, slots: &[usize]) {
        if menu.carried(&*host).flatten().is_some() || slots.is_empty() {
            return;
        }
        let mut st = self.state();
        let mut kinds: Vec<ItemStack> = Vec::new();
        let pieces: Vec<Option<Piece>> = slots
            .iter()
            .map(|&i| {
                let stack = slot(&*host, menu, i)?;
                let kind = match kinds.iter().position(|k| k.is_stackable_with(&stack)) {
                    Some(k) => k,
                    None => {
                        kinds.push(stack.clone());
                        kinds.len() - 1
                    }
                };
                Some(Piece {
                    kind,
                    count: stack.count(),
                })
            })
            .collect();
        let max: Vec<u32> = kinds.iter().map(|k| max_of(&*host, k)).collect();
        let info: Vec<(usize, String, String)> = kinds
            .iter()
            .map(|k| {
                let item = k.item();
                (
                    order(&*host, &mut st, item),
                    name(&*host, &mut st, item),
                    item.id(&*host),
                )
            })
            .collect();
        let rank = |p: Piece| {
            let (order, name, id) = &info[p.kind];
            engine::compute_sort_rank(s.sort_mode, *order, name, id, p.count)
        };
        let clicks = engine::plan(&pieces, &rank, &max);
        let changed = !clicks.is_empty();
        for i in clicks {
            let click = MenuClick::Pickup {
                slot: Some(slots[i]),
                button: MouseButton::Left,
            };
            if !menu.click(host, &[click]) {
                break;
            }
        }
        if changed {
            st.toast.push(
                s,
                format!(
                    "[Inventory Profiles] Sorted ({})",
                    Settings::sort_mode_name(s.sort_mode)
                ),
                engine::COLOR_INFO,
            );
        }
    }

    /// Auto-refill and tool break protection, each frame while playing.
    fn hand(&self, host: &mut dyn ClientHost, s: &Settings) {
        let Some(player) = host.local_player() else {
            self.state().prev_hand.valid = false;
            return;
        };
        let (Some(menu), Some(sel)) = (player.container_menu(&*host), player.selected_slot(&*host))
        else {
            self.state().prev_hand.valid = false;
            return;
        };
        let sel = sel.min(8);
        // The inventory menu: the main inventory is 9..36, the hotbar 36..45.
        let hand_slot = 36 + sel;
        let cur = slot(&*host, menu, hand_slot);

        // 1. Auto-refill when the held hotbar slot just became empty.
        if s.auto_refill {
            let cur_id = cur.as_ref().map(|c| c.item().id(&*host));
            let refill = {
                let st = self.state();
                engine::check_auto_refill_needed(&st.prev_hand, sel, cur_id.as_deref())
            };
            if let Some(target) = refill
                && let Some((best, stack)) = (9..36)
                    .filter_map(|i| slot(&*host, menu, i).map(|st| (i, st)))
                    .filter(|(_, st)| st.item().id(&*host) == target)
                    .max_by_key(|(_, st)| st.count())
            {
                let swap = MenuClick::Swap {
                    slot: best,
                    with: SwapWith::Hotbar(sel as u8),
                };
                if menu.click(host, &[swap]) {
                    let mut st = self.state();
                    let item_name = name(&*host, &mut st, stack.item());
                    st.toast.push(
                        s,
                        format!("[Inventory Profiles] Auto-refilled {item_name}"),
                        engine::COLOR_INFO,
                    );
                }
            }
        }

        // 2. Tool break protection.
        let cur = slot(&*host, menu, hand_slot);
        if s.tool_protection > 0
            && let Some(stack) = cur.as_ref()
        {
            let max = max_damage(&*host, stack);
            if engine::should_protect_tool(stack.damage(), max, s.tool_protection) {
                let remaining = max.saturating_sub(stack.damage());
                let item_name = name(&*host, &mut self.state(), stack.item());
                // An undamaged spare of the same tool in 9..36 first, else an empty slot there.
                let spare = (9..36).find(|&i| {
                    slot(&*host, menu, i).is_some_and(|c| {
                        c.item() == stack.item()
                            && !engine::should_protect_tool(c.damage(), max, s.tool_protection)
                    })
                });
                let empty = (9..36).find(|&i| slot(&*host, menu, i).is_none());
                let safe_hotbar = (0..9).find(|&h| {
                    h != sel
                        && slot(&*host, menu, 36 + h).is_none_or(|c| {
                            let md = max_damage(&*host, &c);
                            !engine::should_protect_tool(c.damage(), md, s.tool_protection)
                        })
                });
                if let Some(target) = spare.or(empty) {
                    let swap = MenuClick::Swap {
                        slot: target,
                        with: SwapWith::Hotbar(sel as u8),
                    };
                    if menu.click(host, &[swap]) {
                        self.state().toast.push(
                            s,
                            format!(
                                "[Inventory Profiles] Saved {item_name} ({remaining} durability left)!"
                            ),
                            engine::COLOR_WARN,
                        );
                    }
                } else if let Some(h) = safe_hotbar {
                    // A client mod can't change the held slot: the tool
                    // trades places with that slot's stack instead.
                    let swap = MenuClick::Swap {
                        slot: 36 + h,
                        with: SwapWith::Hotbar(sel as u8),
                    };
                    if menu.click(host, &[swap]) {
                        self.state().toast.push(
                            s,
                            format!(
                                "[Inventory Profiles] Moved {item_name} to slot {} ({remaining} left)!",
                                h + 1
                            ),
                            engine::COLOR_WARN,
                        );
                    }
                }
            }
        }

        // 3. Record the hand for the next frame.
        let fin = slot(&*host, menu, hand_slot);
        let item = fin
            .as_ref()
            .map(|f| f.item().id(&*host))
            .unwrap_or_default();
        self.state().prev_hand = HandSnapshot {
            valid: true,
            slot: sel,
            item,
            count: fin.map_or(0, |f| f.count()),
        };
    }
}

fn key_name(host: &dyn ClientHost, id: &str, default: &str) -> String {
    host.mod_key(ID, id)
        .map_or_else(|| default.to_owned(), |(key, _)| key)
}

#[mod_api::horizon_mod]
impl Hooks for InventoryProfiles {
    fn register(&self, r: &mut dyn Registrar, _cx: &RegistrationContext) {
        for (id, key) in [
            (engine::KEY_SORT, engine::KEY_SORT_DEFAULT),
            (engine::KEY_QUICK_STACK, engine::KEY_QUICK_STACK_DEFAULT),
        ] {
            r.register_key_mapping(
                KeyRegistration::new(id, key)
                    .label(&format!("inventory_profiles.key.{id}"))
                    .context(KeyConflictContext::Gui),
            );
        }
        for setting in engine::SETTINGS {
            let spec = match setting {
                Setting::Toggle(key, default) => {
                    SettingSpec::toggle(key, format!("inventory_profiles.setting.{key}"), default)
                }
                Setting::Choice(key, options, default) => SettingSpec::choice(
                    key,
                    format!("inventory_profiles.setting.{key}"),
                    options
                        .iter()
                        .map(|o| format!("inventory_profiles.setting.{key}.{o}")),
                    default,
                ),
            };
            let key = spec.key.clone();
            if let Err(e) = r.register_setting(spec) {
                r.registration_refusal(format!("Inventory Profiles setting {key}: {e}"));
            }
        }
    }

    /// Notes the open menu screen for the keys.
    fn screen_render_post(&self, host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
        let screen = event.screen;
        let menu = (screen.id == "inventory")
            .then(|| host.local_player()?.container_menu(host))
            .flatten();
        self.state().open = menu.map(|menu| Open {
            menu,
            storage: storage(screen.menu, screen.slots),
            main: main_slots(screen.menu, screen.slots),
        });
    }

    fn screen_char_typed_pre(
        &self,
        _host: &dyn ClientHost,
        event: &mut ScreenCharTypedPreEvent<'_>,
    ) -> mod_api::Flow {
        self.state().typed_in_field = event.field.is_some();
        mod_api::Flow::Continue
    }

    fn render_frame_pre(&self, host: &mut dyn ClientHost, event: &mut RenderFramePreEvent) {
        self.state().toast.tick(event.elapsed.as_secs_f32());
        if !host.on(ID) {
            self.state().prev_hand.valid = false;
            return;
        }
        let s = settings(&*host);
        if !s.enabled || !host.playing() {
            self.state().prev_hand.valid = false;
            return;
        }
        self.hand(host, &s);
    }

    /// The sort key and the quick-stack key over a menu screen.
    fn input_key(&self, host: &mut dyn ClientHost, event: &mut InputKeyEvent<'_>) {
        let typed = std::mem::take(&mut self.state().typed_in_field);
        if event.action != KeyAction::Press || typed || !host.on(ID) {
            return;
        }
        let s = settings(&*host);
        if !s.enabled {
            return;
        }
        let Some(open) = self.open(&*host) else {
            return;
        };
        let sort_key = key_name(&*host, engine::KEY_SORT, engine::KEY_SORT_DEFAULT);
        let quick_key = key_name(
            &*host,
            engine::KEY_QUICK_STACK,
            engine::KEY_QUICK_STACK_DEFAULT,
        );

        if event.key == sort_key && sort_key != "key.keyboard.unknown" {
            let mouse = host.cursor();
            let hovered = open.menu.slot_boxes(&*host).and_then(|boxes| {
                boxes.iter().position(|b| {
                    b.is_some_and(|[x, y, w, h]| {
                        mouse[0] >= x - 1.0
                            && mouse[0] < x + w + 1.0
                            && mouse[1] >= y - 1.0
                            && mouse[1] < y + h + 1.0
                    })
                })
            });
            let target = if hovered.is_some_and(|h| open.storage.contains(&h))
                || (hovered.is_none() && !open.storage.is_empty())
            {
                open.storage.clone()
            } else {
                open.main.clone()
            };
            self.sort_slots(host, &s, open.menu, &target);
            return;
        }

        // Quick-stack: inventory stacks of items the container already holds go into it.
        if s.quick_stack && event.key == quick_key {
            let menu = open.menu;
            if open.storage.is_empty() || menu.carried(&*host).flatten().is_some() {
                return;
            }
            let existing: HashSet<ItemKey> = open
                .storage
                .iter()
                .filter_map(|&i| slot(&*host, menu, i))
                .map(|st| st.item())
                .collect();
            if existing.is_empty() {
                return;
            }
            let mut moved = 0u32;
            for &i in &open.main {
                if slot(&*host, menu, i).is_some_and(|st| existing.contains(&st.item()))
                    && menu.click(host, &[MenuClick::QuickMove { slot: i }])
                {
                    moved += 1;
                }
            }
            if moved > 0 {
                self.state().toast.push(
                    &s,
                    format!("[Inventory Profiles] Quick-stacked {moved} stack(s) to container"),
                    engine::COLOR_INFO,
                );
            }
        }
    }

    /// The notice above the hotbar.
    fn render_gui_post(&self, host: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
        if !host.on(ID) {
            return;
        }
        let toast = self.state().toast.clone();
        if !toast.showing() {
            return;
        }
        let draw = &mut *event.draw;
        let [width, height] = draw.size();
        let w = draw.text_width(&toast.text);
        let box_w = w + 10.0;
        let box_h = 14.0;
        let x = ((width - box_w) * 0.5).floor();
        let y = (height - 62.0).max(4.0);
        draw.fill([x, y, box_w, box_h], engine::BACKGROUND);
        draw.text(&toast.text, [x + 5.0, y + 3.0], toast.color, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_and_main_slots() {
        assert_eq!(
            storage(Some("minecraft:generic_9x3"), 63),
            (0..27).collect::<Vec<_>>()
        );
        assert!(storage(Some("minecraft:crafting"), 46).is_empty());
        assert!(storage(None, 46).is_empty());
        assert_eq!(main_slots(None, 46), (9..36).collect::<Vec<_>>());
        assert_eq!(
            main_slots(Some("minecraft:generic_9x3"), 63),
            (27..54).collect::<Vec<_>>()
        );
        assert_eq!(
            main_slots(Some("minecraft:crafting"), 46),
            (10..37).collect::<Vec<_>>()
        );
    }
}
