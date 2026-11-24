# What is turned off on Horizon

This fork builds Create for Sunrise against the **Horizon** mod SDK (Nightly `f1bb66f`, Minecraft 26.3). Most of the
Sunrise API has an equivalent on Horizon and was adapted. What has none is turned off and listed below: what the player
loses, the Sunrise API Horizon lacks, and where it is. Code that is off is marked `#[cfg(any())] // Horizon: API missing`
or with a `Horizon:` comment. The engine (`src/engine`) is unchanged.

| In game | Sunrise API missing on Horizon | File |
|---|---|---|
| Toolbox: its own screen (compartment layout, deposit-all and confirm buttons) does not exist | `MenuScreen`, `MenuScreenLayout`, `ScreenSlot`, `register_menu_screen`, `MenuButtonEvent.subject` | `src/glue/blocks.rs` |
| Extendo Grip: aiming does not prefer the block behind a distant entity | `client_pick` hook / `ClientPickEvent` | `src/lib.rs`, `src/glue/equipment.rs` |
| Extendo Grip: shown still in its rest pose (arms, hand and cog built by `create:extendo_grip`), with no extend/punch animation and no spinning cog: Horizon asks for a special model once per stack, not per frame | per-frame `SpecialModelFactory`/`SpecialModelGeometry`/`SpecialModelCollector`, `ItemModelUpdateInput`/`ItemModelLayer` | `src/glue/equipment.rs` (`GripRest`), `assets/create/items/extendo_grip.json` |
| Filter and Attribute Filter: using one does not open its screen, so filters cannot be configured (an already configured filter still works) | `ModMenuOpen`/`open_mod_menu` (menu with extra data), `MenuScreen*`, `MenuPart::menu_clicked` | `src/glue/items/filters.rs`, `src/glue/items/filter_screen.rs`, `src/glue/items/mod.rs` |
| Attribute Filter: "is in creative tab" never matches | `built_creative_tabs`/`CreativeModeTabs` | `src/glue/items/filters.rs`, `src/glue/compat.rs` |
| Schematicannon and Schematic Table: their own screens do not exist (no buttons, no progress bar); shift-click in the inventory still routes items | `MenuScreen*`, `register_menu_screen` | `src/glue/schematics/mod.rs`, `src/glue/schematics/screens.rs` |
| Wand of Symmetry: its screen shows the item flat instead of the rotated 3D model | `Gui3dView`/`Gui3dSubject` | `src/glue/items/symmetry_screen.rs` |
| Wrench, Potato Cannon, Worldshaper (and the Wand of Symmetry's parts): the moving part of the model stays still | per-frame `SpecialModelRenderer` (`extract_argument`/`submit`) | `src/glue/items/spinning.rs` |
| Create's own particles (fan and potato cannon air, indicators, haunting souls): no level light (souls always bright) and no facing of their own | `ParticleBatch`/`ParticleLevelView`/`ParticleFacing`, `register_particle_type` with codecs | `src/glue/compat.rs`, `src/glue/decor/souls.rs` |
| Mechanical Roller, Backtank, Toolbox, Redstone Requester: "on placed" only runs when a player places the item (not a schematic or contraption) | `BlockPart::set_placed_by` (Horizon only has `ItemPart::on_block_placed_by`, without the level) | `src/glue/compat.rs` and those four |
| Potato Cannon projectile: no owner (damage is not credited to the player) | `EntitySpawn.owner`/`LevelRef::spawn_entity` (done with `summon`) | `src/glue/compat.rs` |
| Contraptions, seats, carts: no piston reaction of their own (`IgnoreEntity`) | `PushReaction` parameter of `declare_general_entity` | `src/glue/contraptions/colliders.rs`, `seats.rs`, `trains/carts.rs`, `items/potato.rs` |
| Block sounds: no longer exclude the player who caused them (they may hear them twice) | last parameter (excluded player) of `LevelRef::play_sound` | several |
| (adapted, not cut) Create's block entity data (speed, items, progress...) is no longer in the block entity's NBT but in a chunk attachment (`create:entity_fields`). `/data get block` does not show it; cloning or pick-block with data does not carry it | `block_entity_save_additional`/`load_additional` hooks | `src/glue/entity_data.rs`, `src/glue/mod.rs` |
| Blocks broken by Create (drill, saw...): no break particles or sound | `LevelRef::block_break_effect` | `src/glue/server.rs` |
| Connected textures of multiblocks (tanks, vaults, casing): do not join | `LevelRef::set_connection_group` | `src/glue/mod.rs` |
| `/tick freeze`: kinetic parts do not show the frozen state | `tick_rate_is_frozen` | `src/glue/kinetics.rs` |
| Kinetic parts drawn with a block model: use the spinning fallback drawing instead of the state's model | `RenderAssets::block_state_model` | `src/glue/kinetics.rs` |
| Text built on the server (e.g. display links): shows the key or fallback instead of the translation, and item names by id | `Host::text_string`/`hover_name` exist only on the client | `src/glue/compat.rs`, `src/glue/mod.rs` |
| Keys: "physical key held" uses the mapping state (contraption controls may react differently) | `vanilla_key_mapping`/`mod_key_mapping` (Horizon has `mapping`, `is_down`, `consume_click`) | `src/glue/keys.rs` |
| Create fluids and lava: lava + honey does not make limestone, lava + chocolate does not make scoria | `Registrar::fluid_interaction`/`FluidInteraction` | `src/glue/fluids/lava.rs` |
| Factory Gauge: no collision shape per panel | `collision_shape` hook, `BlockEntityRef::set_shapes`, block field `shapes_from_block_entity` | `src/glue/stock/gauge.rs`, `data/create/horizon/block/factory_gauge.json` |
| Create scaffolding: uses the collision of the `minecraft:scaffolding` template | block field `context_collision` | `data/create/horizon/block/*_scaffolding.json` |
| Moving contraptions (bearings, gantries, carts, elevators): no collision with players or entities (you walk through them) | entity colliders (`set_collider`, `collider`, `set_collider_transform`) | `src/glue/contraptions/colliders.rs`, `src/glue/compat.rs` |
| (adapted, not cut) Player on a belt: carried again by its own client, as in Create, but each tick the belt adds a push to the player's velocity (`ClientHost::push_player`, as the fan does) instead of moving the player itself. The push is worked out against ground friction, so within a few ticks it carries the player at the belt's speed. The player's walking adds on top, so they can still walk along, against or off the belt. On a flat belt the push is sideways only and stops once the feet leave the belt's top, so jumping is untouched. Differences: speed builds up over about a quarter of a second, and after stepping off the player keeps the belt's speed for a few ticks (about two ticks of belt travel). A slow belt blocked by an entity stops pushing but does not stop the player dead. The server did not send the push, so on a LAN or dedicated server Horizon's movement check logs it (and sends the player back if the server is set to). Slopes keep the older approximate push. | `move_local_player_by`, `set_local_player_velocity`, `set_local_player_on_ground` | `src/glue/logistics/belt.rs` |
| Diving Boots: no faster sinking, no higher jump off the bottom, no faster walking underwater | `set_local_player_velocity`/`set_local_player_on_ground` | `src/glue/equipment.rs` |
| Netherite Diving gear: no fire immunity | `set_fire_immunity`/`set_client_fire_immunity` | `src/glue/equipment.rs` |
| Create attacks: do not stop the attacker's sprint | `EntityRef::set_sprinting` | `src/glue/items/sword.rs` |
| Mechanical Arm and belt: do not know an entity's piston reaction or whether it hangs (may grab or push item frames and the like) | `piston_push_reaction`, `is_hanging` | `src/glue/logistics/arms.rs`, `belt.rs` |
| Train carts: position correction is a teleport (no collision on the way) | `EntityRef::move_by` | `src/glue/trains/carts.rs` |
| Potato Cannon: the projectile is not drawn | `EntityRenderState` attachments, `EntityRef::owner` | `src/glue/items/potato.rs` |
| Looping sounds (fan, train horn): do not change pitch or follow their source | `SoundRef::set_pitch`/`move_to` | `src/glue/processing/air.rs`, `src/glue/trains/horn.rs` |
| Wrench and Wand of Symmetry: a rotated or mirrored block does not update its shape from its neighbours | `LevelRef::update_from_neighbour_shapes` | `src/glue/items/wrench.rs`, `src/glue/items/symmetry.rs` |
| Schematicannon: placed blocks do not get "placed by" (item data); the oldest uploaded file is not deleted; "conducts redstone" becomes "is a full cube" | `LevelRef::set_placed_by`, `ConfigFiles::remove`, `is_redstone_conductor` | `src/glue/schematics/` |
| Create copper (shingles, tiles): does not oxidise on its own; sandpaper does not scrape copper or remove wax | `weathering_age`, `previous_weathering_stage`, `unwaxed` | `src/glue/decor/copper.rs`, `src/glue/items/sandpaper.rs` |
| Copycats: the material's friction is not applied; block classes (stairs, trapdoor, bars) are guessed from the id | `BlockPart::friction` with level, `block_class` | `src/glue/copycat/` |
| Toolbox: breaking it drops its contents (instead of keeping them in the item) | `PreRemoveSideEffectsEvent.keep_contents` | `src/glue/processing/containers.rs` |
| Deployer: its punch is not a fully charged attack | `PlayerActionSetup.full_attack_charge` | `src/glue/processing/deployer.rs` |
| Drawn contraptions: do not use the level's light for the whole structure | `BlockEntityDrawContext::level_light` | `src/glue/contraptions/draw.rs` |
| Advancements, toolbelt: the player's persistent data is written as a whole compound (adapted) | `PlayerRef::set_persistent_data_key` | `src/glue/compat.rs` |

## Adaptations that change behaviour

These are not cuts, but they may behave differently in game.

- Content declarations live in `data/create/horizon/` and `assets/create/horizon/`, where Horizon reads them (Sunrise reads `.../sunrise/`).
- `state_for_placement` and `can_survive` get only a `&dyn LevelHost` on Horizon, without the side. The code treats them as server side (`src/glue/compat.rs`, `lifts.rs`, `redstone/parts.rs`, `copycat/parts.rs`). On the client this may queue work only the server consumes.
- Horizon's `block_changed` does not give the change's flags; kinetics always gets an ordinary update (no `KNOWN_SHAPE`). `src/lib.rs`.
- `block_entity_client_tick` goes through an adapter (`compat::ClientTick`) that builds the block entity's view from Horizon's event.
- Item capabilities (depot, belt, funnel, millstone, mixer and basin, crafter, toolbox) go through an adapter (`compat::be_offer`) from Sunrise's "offer" shape to Horizon's "return the handler" shape.
- `ResourceHandler`: `insert_at`/`extract_at` became `insert`/`extract`; Sunrise's index-less `insert`/`extract` became `insert_any`/`extract_any`; `capacity` always gets the resource (no longer an `Option`).
- Particles: `register_particle_type` with codecs became `particle_key` plus a provider registered by name (`compat::ParticleType`).
- Create's debug commands: run through the global `run_mod_command` hook instead of a closure per command.
- Stepping on belts and ejectors: from `BlockPart::step_on` to the global `block_step_on` hook (server only).
- Gearshift and clutch are placed unpowered; the neighbour update powers them.
- Mechanical Press: plays its activation sound as the vanilla sounds Create's `sounds.json` names (anvil land or wool hit, then item break), so it needs nothing from the jar.
- Engineer's Goggles: on the head they use the 3D `create:block/goggles` model, as Create's `GogglesModel` does.

## Checked

- `cargo check`: 0 errors. `cargo horizon-mod build`: builds `target/horizon-mod/create.hmod`, which `cargo horizon-mod check` accepts.
- Built with `horizon-launcher native-build` (optimized) on Horizon Nightly `f1bb66f`, Windows, and played in a new creative world: Create's blocks and items load, kinetics turn (water wheel, cogs, shafts), the press makes iron sheets on a depot with its sound, advancements are awarded, the goggles sit on the face, the Extendo Grip and Wand of Symmetry show their models.
- Not yet checked: `cargo test`, saving and reloading machine state, trains, contraptions, Linux and macOS.
