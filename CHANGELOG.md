# Changelog

What this fork changed on top of [Ola's port](https://github.com/OlaHulleberg/create-sunrise). Each entry was
checked against Create's Java source. "Tested in game" means it was played on Horizon, not only unit-tested.

## 2026-11-26

### Changed
- **Diving gear** is drawn as a layer of the player models, as Create adds it. Third person is unchanged. The
  inventory screen's player still shows none of it: Horizon doesn't run mod layers there (`HORIZON_GAPS.md`,
  issue #1). Tested in game.

### Reverted
- **Brass funnels and smart chutes without a filter** take up to 64 items again, as in the 6.0.10 jar and at the
  Create commit Ola's port follows (`b62e837`). The 99 of 25 November came from Create's development branch, where
  it is part of a larger change to 99-item stacks (22 files: depots, ejectors, belts, basins, tunnels) that no
  release has yet. Taking only the funnel part mixed the two. The fix is kept in the `funnel-99` branch for when
  Ola's port moves to a release that has it.

## 2026-11-25

### Fixed
- **Double sliding doors.** A door placed by a player takes the hinge vanilla's `DoorBlock.getHinge` gives, so two
  doors in a doorway open together, as in Create. Tested in game.
- **Gearshift and clutch** are placed already powered next to a powered block. Before, they waited for the next
  redstone update. Tested in game.
- **Diving gear, worn.** Helmets, backtanks and boots no longer show as copper armour. In third person they are
  drawn with Create's textures, and the backtank sits on the back with its cogs turning. Cardboard armour shows
  too. Tested in game.
- **Backtank filling.** A filling backtank draws air in, and it plays its sound and ring when full. It fills as
  slowly as in Create.
- **Creative tab.** The copper and netherite backtanks are full of air.
- **Brass funnels and smart chutes without a filter** take up to 99 items at a time, not 64. A count is kept when
  the filter changes, as Java keeps it, and older saves load as Java loads them. (Reverted on 26 November.)
- **Wrench on a belt.** A top click on a cased segment or a pulley still takes the casing off or turns the pulley
  back, when the belt slicer can't cut there.

### Tools
- `tools/create-fuzz` runs on Windows: the Java server, the driver and the coverage tool. The driver builds
  against the current engine again.
- Against Create Java's development branch (`37ea4af`, 17 November, newer than Ola's pin), the 124 recorded
  cases and 1,000 random scenarios match, with the funnel change above.

### Merged
- Ola's 32 commits of 23–24 November.

## 2026-11-24

### Fixed
- **Sliding doors:** a train door slides without the vanilla door showing, and the andesite door folds. Tested in
  game.
- **Belts:** clicks on belts; sloped belts with a shaft at the end; a belt carries the player off its end, as
  Create does. The belt connector draws its line between the shafts. Tested in game.
- **Chute:** placing one no longer crashes the game.
- **Light:** the experience block (15) and the lit blaze burner (12, or 9 smouldering) give light.
- **Contraptions:**
  - a turning windmill keeps turning after a reload;
  - Ferris wheels (stabilised bearings) on bearings, clocks and minecarts;
  - pulleys side by side move together;
  - slime and honey stick, the sticky piston pulls back, super glue and the sticker work as in Create;
  - a pulley's rope falls when the pulley breaks;
  - blocks on clock hands work;
  - control panels on trains and minecarts can be clicked;
  - machines keep turning on minecarts and trains.
- **Tracks:** stations, signals and observers are placed by clicking any track: straight, diagonal, sloped or
  curved. A curve can be aimed at and broken on its own. In creative, breaking track drops nothing.
- **Trains:**
  - status messages, such as a missing track;
  - a schedule needs a conductor;
  - arrival bells and whistles, steam, sparks and smoke.

### Ported to Horizon
- The port moved from Sunrise's mod API to Horizon's (`horizon-mod-api`, nightly `f1bb66f`). What the API can't do
  yet is left out and listed in [create/HORIZON_GAPS.md](create/HORIZON_GAPS.md).
- The data tool reads `create/horizon-mod.toml`, and the block and item declarations are in Horizon's format.
- The press plays its sound, the goggles sit on the face, and the extendo grip and the wand of symmetry use their
  models.
