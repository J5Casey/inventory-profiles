//! Inventory Profiles for Horizon: sorting, auto-refill, tool break
//! protection and quick-stack, as a native mod on the mod API
//! (`sunrise-mod-api`, lib `mod_api`). The decisions are in `engine.rs`;
//! this file reads the player's menu and clicks it as a hand would
//! (`MenuRef::click`, ordinary `ContainerClick`s a server accepts).
#![deny(unsafe_code)]

pub mod engine;

use engine::{HandSnapshot, Piece, Setting, Settings, Toast};
use mod_api::{
    ClientHost, DrawContext, Flow, Hooks, InputKeyEvent, ItemKey, ItemStack, KeyAction,
    KeyConflictContext, KeyRegistration, MenuClick, MenuRef, MenuSlotKind, MouseButton,
    MouseButtonPreEvent, Registrar, RegistrationContext, RenderFramePreEvent, RenderGuiPostEvent,
    ScreenCharTypedPreEvent, ScreenRenderPostEvent, SettingSpec, SwapWith, Tag, Text,
};
use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard};

use engine::ID;

/// An open menu screen: its menu and which of its slots are the open
/// container's storage and the player's main inventory (menu order).
#[derive(Clone, Debug)]
struct Open {
    menu: MenuRef,
    /// The container's storage (a chest's, a hopper's, a crafting station's
    /// linked chests): quick-stack's target.
    storage: Vec<usize>,
    /// Whether that storage sorts: not a furnace's, a brewing stand's or a
    /// content mod machine's slots, which take their items by kind.
    sorts: bool,
    main: Vec<usize>,
}

impl Open {
    /// The storage the sort key and the container's button sort.
    fn sortable(&self) -> &[usize] {
        if self.sorts { &self.storage } else { &[] }
    }
}

/// A sort button's side in GUI pixels (the size of Sunrise's own).
const BUTTON: f32 = 9.0;

#[derive(Default)]
struct State {
    prev_hand: HandSnapshot,
    toast: Toast,
    open: Option<Open>,
    /// The last key's character went into a text box (a recipe book's
    /// search): its key is typing, not a command.
    typed_in_field: bool,
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

/// The open menu's storage slots and the player's main inventory's 27
/// (menu order), from what the game says each slot is for
/// (`MenuRef::slot_kinds`: a content mod's menu type too), and whether the
/// storage sorts. `menu` is the menu's type (None: the player's inventory).
///
/// A content mod's menu calls every slot that isn't a result or a crafting
/// grid storage, so a machine's few input slots (Machina's Ember Engine:
/// fuel and coolant) read as storage too. Those don't sort and get no button
/// (it would sit on the machine's gauges): a content mod's menu with fewer
/// than `MACHINE_SLOTS` storage slots is taken for a machine. A hopper, the
/// smallest storage of the game's own, has 5.
///
/// A content mod's own GUI drawn on a chest menu (`ServerHost::open_menu`, a
/// picker or a shop) has storage slots that only show things, so it has no
/// storage at all: no button, no sort, no quick-stack. The game's chest menus
/// are titled by a block's or an entity's name, or by a vanilla `container.x`
/// key; a title that is a mod's own `container.<mod>.<name>` key is the
/// mod's (`own_gui`).
fn layout(
    menu: Option<&str>,
    title: Option<&Text>,
    kinds: &[MenuSlotKind],
) -> (Vec<usize>, bool, Vec<usize>) {
    let gui = own_gui(menu, title);
    let storage: Vec<usize> = (0..kinds.len())
        .filter(|&i| !gui && kinds[i] == MenuSlotKind::Storage)
        .collect();
    let main = (0..kinds.len())
        .filter(|&i| matches!(kinds[i], MenuSlotKind::Player(n) if (9..36).contains(&n)))
        .collect();
    let machine = matches!(
        menu,
        Some(
            "minecraft:furnace"
                | "minecraft:blast_furnace"
                | "minecraft:smoker"
                | "minecraft:brewing_stand"
        )
    );
    let content_machine = menu.is_some_and(|m| {
        !m.starts_with("minecraft:") && !m.starts_with("sunrise:") && storage.len() < MACHINE_SLOTS
    });
    (storage, !(machine || content_machine), main)
}

/// Whether a chest menu is a content mod's own screen: its title is a
/// translation key `container.<mod>.<name>`, which no vanilla container has
/// (`container.chest`, `container.barrel`, `container.hopper`...).
fn own_gui(menu: Option<&str>, title: Option<&Text>) -> bool {
    let chest = menu.is_some_and(|m| m.starts_with("minecraft:generic_9x"));
    let key = title.and_then(|t| match &t.content {
        mod_api::text::TextContent::Translatable { key, .. } => key.strip_prefix("container."),
        _ => None,
    });
    chest && key.is_some_and(|k| k.contains('.'))
}

/// Fewer storage slots than this in a content mod's menu: a machine (`layout`).
const MACHINE_SLOTS: usize = 5;

/// Where a group of slots' sort button goes: above its top row's last
/// slot, as Sunrise's own sorting extra puts it (by a chest's title, by the
/// "Inventory" label). None when the screen shows none of the slots.
fn button_at(boxes: &[Option<[f32; 4]>], slots: &[usize]) -> Option<[f32; 4]> {
    let shown: Vec<[f32; 4]> = slots
        .iter()
        .filter_map(|&i| boxes.get(i).copied().flatten())
        .collect();
    let top = shown.iter().map(|b| b[1]).reduce(f32::min)?;
    let right = shown
        .iter()
        .filter(|b| b[1] == top)
        .map(|b| b[0] + b[2])
        .reduce(f32::max)?;
    Some([right - 8.0, top - 13.0, BUTTON, BUTTON])
}

fn inside(point: [f32; 2], [x, y, w, h]: [f32; 4]) -> bool {
    point[0] >= x && point[0] < x + w && point[1] >= y && point[1] < y + h
}

/// A sort button: a bordered square with three bars, lighter under the
/// pointer (Sunrise's own sorting extra's look).
fn draw_button(draw: &mut dyn DrawContext, [x, y, w, h]: [f32; 4], hovered: bool) {
    let dark = 0xFF37_3737;
    let face = if hovered { 0xFFDC_DCDC } else { 0xFFA0_A0A0 };
    draw.fill([x, y, w, h], dark);
    draw.fill([x + 1.0, y + 1.0, w - 2.0, h - 2.0], face);
    for (i, bar) in [5.0, 4.0, 3.0].into_iter().enumerate() {
        draw.fill([x + 2.0, y + 2.0 + 2.0 * i as f32, bar, 1.0], dark);
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

/// A stack's place in the creative tabs (`creative`, the search tab's
/// stacks): the same stack's place, else its item's first; last for an item
/// the tabs don't show.
fn creative_order(creative: &[ItemStack], stack: &ItemStack) -> usize {
    creative
        .iter()
        .position(|s| s.is_stackable_with(stack))
        .or_else(|| creative.iter().position(|s| s.same_item(stack)))
        .unwrap_or(usize::MAX)
}

fn slot(host: &dyn ClientHost, menu: MenuRef, index: usize) -> Option<ItemStack> {
    menu.slot(host, index).flatten()
}

impl InventoryProfiles {
    /// The open screen's sort buttons and the slots each sorts: the
    /// container's (storage that sorts) and the inventory's.
    fn buttons(&self, host: &dyn ClientHost, open: &Open) -> Vec<([f32; 4], Vec<usize>)> {
        let Some(boxes) = open.menu.slot_boxes(host) else {
            return Vec::new();
        };
        [open.sortable(), &open.main]
            .into_iter()
            .filter_map(|slots| Some((button_at(&boxes, slots)?, slots.to_vec())))
            .collect()
    }

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
        let creative = host.creative_search_items();
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
                (
                    creative_order(&creative, k),
                    host.hover_name(k)
                        .and_then(|t| host.text_string(&t))
                        .unwrap_or_default(),
                    k.item().id(&*host),
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
            self.state().toast.push(
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
                    let item_name = host
                        .hover_name(&stack)
                        .and_then(|t| host.text_string(&t))
                        .unwrap_or_default();
                    self.state().toast.push(
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
                let item_name = host
                    .hover_name(stack)
                    .and_then(|t| host.text_string(&t))
                    .unwrap_or_default();
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

#[mod_api::sunrise_mod]
impl Hooks for InventoryProfiles {
    fn register(&self, r: &mut dyn Registrar, cx: &RegistrationContext) {
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
                cx.refuse(&format!("Inventory Profiles setting {key}: {e}"));
            }
        }
    }

    /// Notes the open menu screen for the keys and the buttons, and draws
    /// the sort buttons.
    fn screen_render_post(&self, host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
        let screen = event.screen;
        let menu = (screen.id == "inventory")
            .then(|| host.local_player()?.container_menu(host))
            .flatten();
        let open = {
            let mut st = self.state();
            if st.open.as_ref().map(|o| o.menu) != menu {
                // The roles stay the same while the menu does.
                st.open = menu.and_then(|menu| {
                    let kinds = menu.slot_kinds(host)?;
                    let (storage, sorts, main) = layout(screen.menu, screen.title, &kinds);
                    Some(Open {
                        menu,
                        storage,
                        sorts,
                        main,
                    })
                });
            }
            st.open.clone()
        };
        let Some(open) = open else {
            return;
        };
        if !host.on(ID) {
            return;
        }
        let s = settings(host);
        if !s.enabled || !s.sort_buttons {
            return;
        }
        for (at, _) in self.buttons(host, &open) {
            draw_button(event.draw, at, inside(screen.mouse, at));
        }
    }

    /// A left press on a sort button sorts its slots, and goes no further.
    fn mouse_button_pre(&self, host: &mut dyn ClientHost, event: &mut MouseButtonPreEvent) -> Flow {
        if !event.pressed || event.button != 0 || !host.on(ID) {
            return Flow::Continue;
        }
        let s = settings(&*host);
        if !s.enabled || !s.sort_buttons {
            return Flow::Continue;
        }
        let Some(open) = self.open(&*host) else {
            return Flow::Continue;
        };
        let mouse = host.cursor();
        let Some((_, slots)) = self
            .buttons(&*host, &open)
            .into_iter()
            .find(|(at, _)| inside(mouse, *at))
        else {
            return Flow::Continue;
        };
        self.sort_slots(host, &s, open.menu, &slots);
        Flow::Cancel
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
            let storage = open.sortable();
            let target = if hovered.is_some_and(|h| storage.contains(&h))
                || (hovered.is_none() && !storage.is_empty())
            {
                storage.to_vec()
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

    use MenuSlotKind::{CraftingGrid, Other, Player, Result, Storage};

    /// The 36 player slots every container menu ends with.
    fn player() -> Vec<MenuSlotKind> {
        (9..36).chain(0..9).map(Player).collect()
    }

    #[test]
    fn layout_reads_the_slot_roles() {
        // A content mod's crafting station: grid, result, two linked chests' slots.
        let mut station = vec![CraftingGrid; 9];
        station.push(Result);
        station.extend([Storage; 54]);
        station.extend(player());
        let (storage, sorts, main) = layout(Some("braid:crafting_station"), None, &station);
        assert_eq!(storage, (10..64).collect::<Vec<_>>());
        assert!(sorts);
        assert_eq!(main, (64..91).collect::<Vec<_>>());
        // A furnace's slots are storage for quick-stack but don't sort.
        let mut furnace = vec![Storage, Storage, Result];
        furnace.extend(player());
        let (storage, sorts, main) = layout(Some("minecraft:furnace"), None, &furnace);
        assert_eq!(storage, [0, 1]);
        assert!(!sorts);
        assert_eq!(main, (3..30).collect::<Vec<_>>());
        // The inventory: no storage; armour (Player 36..) is not the main inventory.
        let mut inventory = vec![
            Result,
            CraftingGrid,
            CraftingGrid,
            CraftingGrid,
            CraftingGrid,
        ];
        inventory.extend((36..40).rev().map(Player));
        inventory.extend(player());
        inventory.push(Player(40));
        let (storage, _, main) = layout(None, None, &inventory);
        assert!(storage.is_empty());
        assert_eq!(main, (9..36).collect::<Vec<_>>());
        assert!(layout(Some("minecraft:anvil"), None, &[Other; 39]).0.is_empty());
        // A content mod's machine: Machina's Ember Engine (fuel, coolant) and
        // Essence Press (two inputs, a result) quick-stack but don't sort.
        let mut engine = vec![Storage, Storage];
        engine.extend(player());
        let (storage, sorts, _) = layout(Some("machina:ember_engine"), None, &engine);
        assert_eq!(storage, [0, 1]);
        assert!(!sorts);
        let mut press = vec![Storage, Storage, Result];
        press.extend(player());
        assert!(!layout(Some("machina:essence_press"), None, &press).1);
        // A content mod's own screen on a chest menu: no storage.
        let mut gui = vec![Storage; 36];
        gui.extend(player());
        let own = Text::translate("container.chisel.chisel", Vec::new());
        let chest = Text::translate("container.chest", Vec::new());
        assert!(layout(Some("minecraft:generic_9x4"), Some(&own), &gui).0.is_empty());
        assert_eq!(layout(Some("minecraft:generic_9x4"), Some(&chest), &gui).0.len(), 36);
        assert_eq!(layout(Some("minecraft:generic_9x4"), None, &gui).0.len(), 36);
        // A hopper's 5 and a llama's 3-slot chest still sort.
        let mut hopper = vec![Storage; 5];
        hopper.extend(player());
        assert!(layout(Some("minecraft:hopper"), None, &hopper).1);
        let mut llama = vec![Other, Other, Storage, Storage, Storage];
        llama.extend(player());
        assert!(layout(Some("sunrise:mount2"), None, &llama).1);
    }

    #[test]
    fn buttons_sit_above_the_top_rows_last_slot() {
        // A chest's 27 slots at (8 + 18c, 18 + 18r) in a panel at (100, 50).
        let boxes: Vec<Option<[f32; 4]>> = (0..27)
            .map(|i| {
                let (c, r) = ((i % 9) as f32, (i / 9) as f32);
                Some([108.0 + 18.0 * c, 68.0 + 18.0 * r, 16.0, 16.0])
            })
            .collect();
        let slots: Vec<usize> = (0..27).collect();
        // Sunrise's own: (160, 5) from the panel.
        assert_eq!(
            button_at(&boxes, &slots),
            Some([260.0, 55.0, BUTTON, BUTTON])
        );
        assert_eq!(button_at(&[None, None], &[0, 1]), None);
        assert!(inside([260.0, 55.0], [260.0, 55.0, BUTTON, BUTTON]));
        assert!(!inside([269.0, 55.0], [260.0, 55.0, BUTTON, BUTTON]));
    }
}
