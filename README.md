# Inventory Profiles for Horizon

A native **Inventory Profiles Next (IPN) / Inventory Sorter** mod for [Horizon](https://github.com/arismoko/horizon)
(Minecraft: Java Edition 26.3 rewrite in Rust), on the arismoko fork's mod API.

## Features

- **Sorting (`R`):**
  - Merges partial stacks and sorts the container under the cursor (`Chests`, `Barrels`, `Shulker Boxes`, `Ender
    Chests`, hoppers, dispensers, a mount's chest) or your main inventory (`9..36`), on every menu screen (crafting
    tables and workstations included: there it sorts your inventory).
  - **4 Sort Modes:** Category (the item registry's order: a mod can't read the creative tabs'), Alphabetical
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

## On arismoko's mod API

API-only (no game internals), pinned in `horizon-mod.toml` to J5Casey/horizon `braid/ari-base` `24715f9`. Hooks:
`render_frame_pre` (auto-refill and tool protection while playing), `input_key` (the keys; `screen_char_typed_pre`
tells typing into the recipe book's search apart), `screen_render_post` (which menu is open), `render_gui_post` (the
notice above the hotbar).

What changed from the injection build: the sort buttons are gone (they were Horizon's sorting extra, hooked through a
private function); the key sorts. Which slots are a container's storage comes from the menu's type id, so a content
mod's menu type (a crafting station's linked chests) gets no quick-stack and sorts the inventory only: the mod API
gives a client no read of what a mod menu's slots are for. The sort plan is Horizon's `inv_sort::plan`, copied into
`src/engine.rs` (a mod can't call it).

## Development

```sh
(cd engine && cargo test && cargo clippy --all-targets -- -D warnings)
# against a local checkout of the pinned base: mkdir -p .horizon/sdk && ln -s <checkout> .horizon/sdk/api
cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

## Author
Developed by **CayMac**.

## License
[MIT](LICENSE)
