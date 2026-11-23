//! `AllFluids.registerFluidInteractions`: source lava becomes obsidian; flowing lava
//! becomes limestone beside honey and scoria beside chocolate.
//!
//! Horizon: no declared fluid interactions (`Registrar::fluid_interaction`); lava meets honey and
//! chocolate as it meets any other fluid. See `HORIZON_GAPS.md`.

use mod_api::Registrar;

pub(super) fn register(_r: &mut dyn Registrar) {}
