# Inventory Profiles for Horizon

A native **Inventory Profiles Next (IPN) / Inventory Sorter** mod for [Horizon](https://github.com/arismoko/horizon)
(Minecraft: Java Edition 26.3 rewrite in Rust), on the arismoko fork's mod API.

## Features

- **Sorting (`R` and the sort buttons):**
  - Merges partial stacks and sorts the container under the cursor (`Chests`, `Barrels`, `Shulker Boxes`, `Ender
    Chests`, hoppers, dispensers, a mount's chest) or your main inventory (`9..36`), on every menu screen (crafting
    tables and workstations included: there it sorts your inventory).
  - **Sort buttons:** a small button above a container's storage (by its title) and one above your inventory
    (by the "Inventory" label) sort that part; turn them off with "UI Sort Buttons".
  - **4 Sort Modes:** Category (the creative tabs' order, as Inventory Profiles Next sorts), Alphabetical
    (`A-Z`), Item Count (`High to Low`), or Item ID.
- **Auto-Refill Hand Stacks:**
  - Replaces depleted blocks, food, torches, or projectiles in your main hand with a matching stack from your
    inventory as soon as the stack runs out.
- **Tool Break Protection (Save My Tools):**
  - Swaps out low-durability tools (`<= 2`, `5`, or `10` durability left) with a fresh backup tool or moves them
    safely into your inventory. With no room there, the tool trades places with a safe hotbar slot's stack (a client
    mod can't change which hotbar slot you hold).
- **Quick-Stack to Chest (`G`):**
  - Press `G` inside an open container to transfer every stack of your main inventory whose item the container
    already holds.

Settings are on the mod's page in **Options > Mods**, its keys in **Controls**. Every click it makes is an ordinary
menu click (`MenuRef::click`), so servers accept it.

## On the Sunrise mod API

API-only (no game internals), pinned in `sunrise-mod.toml` to J5Casey/sunrise `braid/sun-base` (the base every Braid
mod pins). Hooks: `render_frame_pre` (auto-refill and tool protection while playing), `input_key` (the keys;
`screen_char_typed_pre` tells typing into the recipe book's search apart), `screen_render_post` (which menu is open,
and the sort buttons drawn), `mouse_button_pre` (a press on a sort button), `render_gui_post` (the notice above the
hotbar).

Which slots are a container's storage comes from the game (`MenuRef::slot_kinds`), so a content mod's menu (a crafting
station's linked chests) sorts and takes quick-stacks like a chest; a furnace's and a brewing stand's slots take
quick-stacks but don't sort, nor do a content mod's machine's (a mod menu with fewer than 5 storage slots, such as
Machina's Ember Engine and Essence Press, whose inputs the game also calls storage), so no button sits on their gauges. The Category order is the creative search tab's (`ClientHost::creative_search_items`,
sunrise-org/sunrise#1516), ranked as the injection build did: a stack's own place, else its item's first. The buttons
are the mod's own, drawn where and as Sunrise's sorting extra draws its own. The sort plan is the game's
`inv_sort::plan`, copied into `src/engine.rs` (a mod can't call it).

## Development

```sh
(cd engine && cargo test && cargo clippy --all-targets -- -D warnings)
# against a local checkout of the pinned base: mkdir -p .sunrise/sdk && ln -s <checkout> .sunrise/sdk/api
cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

## Author
Developed by **CayMac**.

## License
[MIT](LICENSE)
