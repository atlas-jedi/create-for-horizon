<p align="center">
  <img src="docs/media/banner.webp" alt="Create for Sunrise and Horizon: a copper locomotive on a stone viaduct at sunset beside a Create factory and windmill" width="100%">
</p>

<h1 align="center">Create for Horizon</h1>

<p align="center">
  <b><a href="https://github.com/Creators-of-Create/Create">Create</a> 6.0.11 as a native Rust mod for
  <a href="https://playhorizon.dev">Horizon</a>, Minecraft 26.3 rewritten in Rust.</b><br>
  No Java at build time or play time.
</p>

<p align="center">
  <a href="#install"><b>Install</b></a> ·
  <a href="#status"><b>Status</b></a> ·
  <a href="create/HORIZON_GAPS.md"><b>Known gaps</b></a> ·
  <a href="#how-it-stays-faithful"><b>Fidelity</b></a> ·
  <a href="CHANGELOG.md"><b>Changelog</b></a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/licence-MIT-E2AC52" alt="Licence: MIT"></a>
  <a href="https://modrinth.com/mod/create"><img src="https://img.shields.io/badge/Create-6.0.11-966828" alt="Create 6.0.11"></a>
  <a href="https://playhorizon.dev"><img src="https://img.shields.io/badge/Horizon-nightly%20f1bb66f-6A5ACD" alt="Horizon nightly f1bb66f"></a>
</p>

This is a fork of **[OlaHulleberg/create-sunrise](https://github.com/OlaHulleberg/create-sunrise)**, Ola
Hulleberg's port of Create to Rust, which was built for Sunrise. The porting is Ola's work. This fork:

- runs it on Horizon's mod SDK (`horizon-mod-api`);
- keeps comparing it with Create's Java source and fixes what differs;
- lists what Horizon can't do yet, so nothing is hidden.

Create is the mod by [the Create Team](https://github.com/Creators-of-Create): shafts and cogs, belts and
presses, contraptions and trains.

<table>
  <tr>
    <td width="50%"><img src="docs/media/readme/anim-factory.webp" alt="Belts, mechanical arms, mixers and presses working in a factory hall"><br><sub>The factory at work</sub></td>
    <td width="50%"><img src="docs/media/readme/anim-train-arrival.webp" alt="A train arriving over a viaduct"><br><sub>A train on its schedule</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/media/readme/anim-windmill.webp" alt="Windmill sails turning"><br><sub>Windmill bearing</sub></td>
    <td width="50%"><img src="docs/media/readme/anim-water-wheel.webp" alt="A large water wheel turning in a stream"><br><sub>Water wheel and gearing</sub></td>
  </tr>
</table>

<sub>Banner and animations by Ola Hulleberg, captured on Sunrise with the same engine this fork runs. On Horizon,
trains and contraptions are not played yet: see [Status](#status). Create's textures and models, © the Create
Team, come from its official jar.</sub>

## Status

Upstream ports every part of Create. On Horizon today:

| Area | State |
|---|---|
| Kinetics: shafts, cogs, gearboxes, water wheels, motors, stress | Tested in game |
| Belts: flat and sloped, connecting, carrying items and players | Tested in game |
| Pressing and depots | Tested in game |
| Sliding and train doors, double doors | Tested in game |
| Gearshifts and clutches | Tested in game |
| Diving gear worn (third person), cardboard armour | Tested in game |
| Processing: mixers, millstones, crushing wheels, saws, drills, fans, deployers, spouts | Ported and unit-tested, not yet played on Horizon |
| Logistics: funnels, chutes, tunnels, mechanical arms, ejectors | Ported and unit-tested, not yet played on Horizon |
| Contraptions: bearings, windmills, pistons, pulleys, gantries, clocks, minecarts, Ferris wheels | Ported and unit-tested, not yet played on Horizon |
| Trains: tracks, stations, signals, schedules, conductors, train status | Ported and unit-tested, not yet played on Horizon |
| Fluids, packages and stock, redstone and display, equipment | Ported from upstream, not yet played on Horizon |

Some things need API that Horizon doesn't have yet. These are the ones you notice most:

- You walk through moving contraptions and trains.
- The filter, toolbox and schematic screens don't open.
- A belt carrying you bobs the camera as if you were walking.
- Trains make no rolling sound.
- A curve on a slope has no tilted track block.

The full list, with the API each one needs: **[create/HORIZON_GAPS.md](create/HORIZON_GAPS.md)**.

What changed and when: **[CHANGELOG.md](CHANGELOG.md)**.

## Install

You need:

- **The [Horizon launcher](https://playhorizon.dev)**, with a Horizon instance on nightly `f1bb66f`
  (2026-11-22), the build this mod is pinned to in [create/horizon-mod.toml](create/horizon-mod.toml).
- **A Minecraft: Java Edition account** that has played that instance once.
- **Git.**

Then build it into your instance:

```sh
git clone -b horizon https://github.com/atlas-jedi/create-for-horizon
cd create-for-horizon/create
horizon-launcher native-build . --instance "My Instance" --trust-native-code
```

The launcher compiles the mod with its own Rust toolchain, against your instance's exact Horizon. Then start the
instance as usual. Native mods run with your rights, which is why the launcher asks for `--trust-native-code`.
Try it in a new world first.

> [!NOTE]
> Create's textures, models and sounds are All Rights Reserved, so they are not in this repository. The launcher
> downloads Create's official jar (1.21.1-6.0.10) to your own machine from Modrinth, where the Create Team
> publishes it, and checks it against the SHA-512 pinned in `horizon-mod.toml`.

## How it stays faithful

- **Line by line from Java.** The engine (`create/src/engine`) is Create's rules translated from its Java source,
  with no game types. The glue (`create/src/glue`) only reads the world, calls the engine and writes back.
- **Tests.** About 1,000 engine tests and 69 data-generator tests. A guard test fails if a generated file is
  edited by hand.
- **Differential fuzzing.** [tools/create-fuzz](tools/create-fuzz/README.md) runs the same random setups in
  Create's Java, on a headless server, and in the engine. When they differ, it shrinks the case to the smallest
  one that still differs. On 2026-11-25, its 124 recorded cases and 1,000 random scenarios (kinetics, belts,
  funnels, chutes, depots, processing, fluids) matched Create 6.0.11.
- **Gaps written down.** When Horizon can't do something, the mod leaves it out and
  [HORIZON_GAPS.md](create/HORIZON_GAPS.md) says what and why. There are no stubs that pretend.

Performance against Java Create has not been measured yet.

## Build from source

Install Rust 1.99.1 and Horizon's `cargo-horizon-mod` tool (see Horizon's
[native mod guide](https://playhorizon.dev/docs/native-mods)), then:

```sh
cd create
cargo horizon-mod build                 # fetches the pinned SDK into create/.horizon/, validates the content
cargo check
cd src/engine && cargo test --lib       # the engine's tests
cd ../../../tools/create-data
CREATE_JAR=/path/to/create-1.21.1-6.0.10.jar CREATE_CHECKOUT=/path/to/Create cargo test
```

After a change to Create's data, regenerate the committed tables and declarations from a Create checkout. Build
`tools/create-data`, then run its `compile` into an empty folder and compare it with the repository before
copying it over.

### Layout

```text
create/                 The native mod: what the launcher builds
  horizon-mod.toml      Identity, the pinned Horizon build and Create's jar
  HORIZON_GAPS.md       What Horizon can't do yet, and what the mod leaves out because of it
  src/lib.rs            The one `impl Hooks`, handing each hook to every subsystem
  src/glue/             The game glue on the mod API, one module per subsystem
  src/engine/           create-engine: Create's rules as plain Rust, no game types
  data/, assets/        Generated block, item and fluid declarations
tools/create-data/      compile: tables and declarations from Create's source and data
tools/create-fuzz/      Differential fuzzer: the engine against Create's Java (development only)
docs/                   Architecture and media
```

## Licences

This repository is [MIT](LICENSE). It translates Create's code and compiles its data, which are MIT too.

Create's textures, models and sounds are All Rights Reserved, © the Create Team. They are not in this repository
and are never shipped from it: they come from Create's official jar. Details:
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

This is an unofficial port. Create is made by [the Create Team](https://github.com/Creators-of-Create). The port
to Rust is [Ola Hulleberg's](https://github.com/OlaHulleberg/create-sunrise).

## Contributing

Issues and pull requests about running on Horizon are welcome
[here](https://github.com/atlas-jedi/create-for-horizon/issues). Issues about the port itself, outside Horizon,
belong [upstream](https://github.com/OlaHulleberg/create-sunrise). Never commit Create's assets.
