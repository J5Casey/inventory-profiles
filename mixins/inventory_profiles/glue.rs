// Glue between Horizon's `Game` and `mod_inventory_profiles` runtime state.
use crate::game::{Game, NativeSlotRole, Screen};
use crate::game::mod_inventory_profiles as m;
use crate::game::native_hooks::NativeMod;
use core::ops::ControlFlow;
use horizon_client::{
    extras_hud::Corners,
    hud::HudBatch,
    inv_sort,
    items::ItemStack,
    menu::Input,
    stations::hover_name,
};
use std::collections::BTreeSet;
use std::time::Duration;

pub(super) fn on_frame(game: &mut Game<'_>, elapsed: Duration) {
    let dt = elapsed.as_secs_f32();
    m::with_state(|st| {
        if st.toast_timer > 0.0 {
            st.toast_timer = (st.toast_timer - dt).max(0.0);
        }
    });

    if !game.native_on(m::ID) {
        game.settings.extras.sort = false;
        m::with_state(|st| st.prev_hand.valid = false);
        return;
    }

    let s = {
        let g = &*game;
        m::settings(|key| g.native_setting(m::ID, key))
    };

    game.settings.extras.sort = s.enabled && s.sort_buttons;
    game.settings.extras.sort_key =
        game.native_mod_key(m::ID, m::KEY_SORT, m::KEY_SORT_DEFAULT);

    if !s.enabled || !game.native_playing() {
        m::with_state(|st| st.prev_hand.valid = false);
        return;
    }

    let sel = game.selected.min(8);
    let cur_stack = game.slot_view(sel);

    // 1. Check Auto-Refill if active hotbar slot just became empty
    if s.auto_refill {
        let refill_item = m::with_state(|st| {
            m::check_auto_refill_needed(
                &st.prev_hand,
                sel,
                cur_stack.as_ref().map(|st_item| st_item.item.as_ref()),
            )
        });
        if let Some(target_id) = refill_item {
            if let Some((best_slot, best_stack)) = (9..36)
                .filter_map(|slot| game.slot_view(slot).map(|st| (slot, st)))
                .filter(|(_, st)| st.item.as_ref() == target_id)
                .max_by_key(|(_, st)| st.count)
            {
                let item_name = hover_name(&game.pack.names, &best_stack);
                if game.tweak_click(best_slot, sel as i32, Input::Swap) {
                    m::push_toast(
                        format!("[Inventory Profiles] Auto-refilled {item_name}"),
                        m::COLOR_INFO,
                    );
                }
            }
        }
    }

    // Re-read current hand stack after potential auto-refill
    let cur_stack = game.slot_view(sel);

    // 2. Check Tool Break Protection
    if s.tool_protection > 0 {
        if let Some(stack) = cur_stack.as_ref() {
            let max_damage = game.items().get(&stack.item).map_or(0, |i| i.max_damage);
            if m::should_protect_tool(stack.damage, max_damage, s.tool_protection) {
                let remaining = max_damage.saturating_sub(stack.damage);
                let item_name = hover_name(&game.pack.names, stack);
                // Look for an undamaged replacement of the same tool in 9..36 first, else an empty slot in 9..36
                let replacement_slot = (9..36).find(|&slot| {
                    game.slot_view(slot).is_some_and(|cand| {
                        cand.item == stack.item
                            && !m::should_protect_tool(cand.damage, max_damage, s.tool_protection)
                    })
                });
                let empty_slot = (9..36).find(|&slot| game.slot_view(slot).is_none());

                if let Some(target_slot) = replacement_slot.or(empty_slot) {
                    if game.tweak_click(target_slot, sel as i32, Input::Swap) {
                        m::push_toast(
                            format!(
                                "[Inventory Profiles] Saved {item_name} ({remaining} durability left)!"
                            ),
                            m::COLOR_WARN,
                        );
                    }
                } else if let Some(safe_hotbar) = (0..9).find(|&h| {
                    h != sel
                        && game.slot_view(h).is_none_or(|cand| {
                            let md = game.items().get(&cand.item).map_or(0, |i| i.max_damage);
                            !m::should_protect_tool(cand.damage, md, s.tool_protection)
                        })
                }) {
                    game.selected = safe_hotbar;
                    m::push_toast(
                        format!(
                            "[Inventory Profiles] Switched slot to save {item_name} ({remaining} left)!"
                        ),
                        m::COLOR_WARN,
                    );
                }
            }
        }
    }

    // 3. Record hand snapshot for next frame
    let final_stack = game.slot_view(sel);
    m::with_state(|st| {
        st.prev_hand = match final_stack {
            Some(stack) => m::HandSnapshot {
                valid: true,
                slot: sel,
                item: stack.item.to_string(),
                count: stack.count,
            },
            None => m::HandSnapshot {
                valid: true,
                slot: sel,
                item: String::new(),
                count: 0,
            },
        };
    });
}

pub(super) fn on_sort_slots(game: &mut Game<'_>, slots: &[usize]) -> ControlFlow<()> {
    if !game.native_on(m::ID) {
        return ControlFlow::Continue(());
    }
    let s = {
        let g = &*game;
        m::settings(|key| g.native_setting(m::ID, key))
    };
    if !s.enabled {
        return ControlFlow::Continue(());
    }
    sort_slots_with_mode(game, slots, s.sort_mode);
    ControlFlow::Break(())
}

fn sort_slots_with_mode(game: &mut Game<'_>, slots: &[usize], sort_mode: u8) {
    if game.carried.is_some() || slots.is_empty() {
        return;
    }
    let views: Vec<Option<ItemStack>> = slots.iter().map(|&s| game.slot_view(s)).collect();
    let clicks = {
        let rank = |stack: &ItemStack| {
            let order = game.creative_screen.order_of(stack).unwrap_or(usize::MAX);
            let display = hover_name(&game.pack.names, stack);
            m::compute_sort_rank(sort_mode, order, &display, &stack.item, stack.count)
        };
        let max = |stack: &ItemStack| game.max_stack(stack);
        inv_sort::plan(&views, &rank, &max)
    };
    let changed = !clicks.is_empty();
    for i in clicks {
        game.tweak_click(slots[i], 0, Input::Pickup);
    }
    if changed {
        m::push_toast(
            format!(
                "[Inventory Profiles] Sorted ({})",
                m::Settings::sort_mode_name(sort_mode)
            ),
            m::COLOR_INFO,
        );
    }
}

/// The open menu's storage slots (a chest's, a hopper's, a mod menu's linked
/// slots), as Horizon says what each slot is for: empty on the inventory
/// screen, a crafting table or any screen without storage.
fn open_container_slots(game: &Game<'_>) -> Vec<usize> {
    game.native_slot_roles()
        .into_iter()
        .filter(|&(_, role)| role == NativeSlotRole::Storage)
        .map(|(slot, _)| slot)
        .collect()
}

pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !pressed || !game.native_on(m::ID) {
        return;
    }
    let s = {
        let g = &*game;
        m::settings(|k| g.native_setting(m::ID, k))
    };
    if !s.enabled || game.screen != Screen::Inventory || game.recipe_search_focused() {
        return;
    }

    let sort_key = game.native_mod_key(m::ID, m::KEY_SORT, m::KEY_SORT_DEFAULT);
    let quick_key = game.native_mod_key(m::ID, m::KEY_QUICK_STACK, m::KEY_QUICK_STACK_DEFAULT);

    // If UI sort buttons are turned off in settings, handle R-key sorting directly here
    if !s.sort_buttons && key == sort_key {
        let container = open_container_slots(game);
        let hovered = game.inventory_target().0;
        let target_slots: Vec<usize> = if hovered.is_some_and(|h| container.contains(&h))
            || (hovered.is_none() && !container.is_empty())
        {
            container
        } else {
            (9..36).collect()
        };
        sort_slots_with_mode(game, &target_slots, s.sort_mode);
        return;
    }

    // Quick-Stack matching items from player inventory (9..36) into the open menu's storage
    if s.quick_stack && key == quick_key {
        let container = open_container_slots(game);
        if container.is_empty() || game.carried.is_some() {
            return;
        }
        let mut existing_items = BTreeSet::new();
        for &c_slot in &container {
            if let Some(st) = game.slot_view(c_slot) {
                existing_items.insert(st.item.to_string());
            }
        }
        if existing_items.is_empty() {
            return;
        }
        let mut moved = 0u32;
        for inv_slot in 9..36 {
            if let Some(st) = game.slot_view(inv_slot) {
                if existing_items.contains(st.item.as_ref())
                    && game.tweak_click(inv_slot, 0, Input::QuickMove)
                {
                    moved += 1;
                }
            }
        }
        if moved > 0 {
            m::push_toast(
                format!("[Inventory Profiles] Quick-stacked {moved} stack(s) to container"),
                m::COLOR_INFO,
            );
        }
    }
}

pub(super) fn on_hud(game: &mut Game<'_>, batch: &mut HudBatch, _corners: &mut Corners) {
    if !game.native_on(m::ID) {
        return;
    }
    let (text, color, timer) =
        m::with_state(|st| (st.toast_text.clone(), st.toast_color, st.toast_timer));
    if timer <= 0.0 || text.is_empty() {
        return;
    }
    let w = game.native_text_width(&text);
    let box_w = w + 10.0;
    let box_h = 14.0;
    let x = ((batch.width - box_w) * 0.5).floor();
    let y = (batch.height - 62.0).max(4.0);
    game.native_fill(batch, [x, y, box_w, box_h], m::BACKGROUND);
    game.native_text(batch, &text, [x + 5.0, y + 3.0], color, true);
}

pub(super) fn on_list(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: m::ID.into(),
        title: format!("{} {}", m::NAME, m::VERSION),
        description: m::DESCRIPTION.into(),
        keys: vec![
            [
                m::KEY_SORT.into(),
                m::KEY_SORT_LABEL.into(),
                m::KEY_SORT_DEFAULT.into(),
            ],
            [
                m::KEY_QUICK_STACK.into(),
                m::KEY_QUICK_STACK_LABEL.into(),
                m::KEY_QUICK_STACK_DEFAULT.into(),
            ],
        ],
    });
}

pub(super) fn on_settings(game: &mut Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id != m::ID {
        return;
    }
    let g = &*game;
    let settings = m::settings(|key| g.native_setting(m::ID, key));
    buttons.extend(settings.labels());
}

pub(super) fn on_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != m::ID {
        return;
    }
    let mut settings = {
        let g = &*game;
        m::settings(|key| g.native_setting(m::ID, key))
    };
    settings.click(button);
    m::set_settings(settings);
    for (key, value) in settings.stored() {
        game.native_set_setting(m::ID, key, &value);
    }
}
