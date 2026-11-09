# Inventory Profiles for Horizon

A native **Inventory Profiles Next (IPN) / Inventory Sorter** mod (`.hnmod`) for [Horizon](https://github.com/horizon-rust/horizon) (Minecraft: Java Edition 26.3 rewrite in Rust).

## Features

- **1:1 Inventory & Chest Sorting (`R` Keybind + UI Sort Buttons):**
  - Automatically merges partial stacks and sorts containers (`Chests`, `Barrels`, `Shulker Boxes`, `Ender Chests`, hoppers, dispensers, a mod menu's storage slots) or your main inventory (`9..36`), on every menu screen (crafting tables and workstations included: there the inventory's button sorts your inventory).
  - Which slots are storage comes from Horizon (`native_slot_roles`, horizon-rust/horizon#13033), so a content mod's menu type works without a list of menu ids.
  - **4 Sort Modes:** Category (Creative Tab order), Alphabetical (`A–Z`), Item Count (`High to Low`), or Item ID.
- **Auto-Refill Hand Stacks:**
  - Automatically replaces depleted blocks, food, torches, or projectiles in your main hand with a matching stack from your inventory as soon as the stack runs out.
- **Tool Break Protection (Save My Tools):**
  - Prevents expensive tools, swords, and bows from breaking! Automatically swaps out low-durability tools (`<= 2`, `5`, or `10` durability left) with a fresh backup tool or moves them safely into your inventory.
- **Quick-Stack to Chest (`G` Keybind):**
  - Press `G` inside an open container to instantly transfer all items from your inventory that match items already stored inside the chest (any menu's storage slots: a crafting station's linked chests too).

## Author
Developed by **CayMac**.

## License
[MIT](LICENSE)
