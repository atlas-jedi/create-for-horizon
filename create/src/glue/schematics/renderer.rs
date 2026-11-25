//! The deployed schematic's blocks (`SchematicRenderer` and `SchematicHandler.setupRenderer`):
//! loads the schematic's `.nbt` file on the client, builds its unmirrored, front-back mirrored
//! and left-right mirrored placements (`Mirror::None`, `Mirror::FrontBack`, `Mirror::LeftRight`),
//! resolves each distinct block state's baked model once per load (`RenderAssets::block_state_model`)
//! and draws its instances under `SchematicTransformation` (`WorldDrawContext::draw_model_instances`).
//!
//! Block entities inside the preview (`BlockEntityRenderHelper`) are not drawn here; static
//! block models draw with world light sampled at each block's transformed cell and face culling
//! between solid-render neighbours inside the schematic.
//!
//! Horizon: no block state's baked model (`RenderAssets::block_state_model`) and no per-copy
//! face culling (`ModelInstance::with_hidden_faces`), so nothing resolves and the preview draws
//! nothing (`compat::ext`, see HORIZON_GAPS.md).

#[allow(unused_imports)] use crate::glue::compat::ext::*;
use super::transform::Placing;
use crate::glue::names::Named as _;
use create_engine::schematic::{self as sch, Mirror, Rotation, Structure};
use mod_api::{
    BlockState, ClientHost, Direction, HostBlockStates, LightCoords, LightLayer, ModelInstance,
    ModelRef, Overlay, RenderLevelStage, RenderLevelStageEvent,
};
use std::collections::{HashMap, HashSet};

/// One block cell in a mirrored schematic variant: local coordinates and culled face mask.
type Cell = ([i32; 3], u8);

/// One of `SchematicHandler`'s three prebuilt views (`None`, `FrontBack`, `LeftRight`).
struct Variant {
    /// Grouped by `BlockState` with its block light emission (`0..=15`) so each state resolves
    /// one `ModelRef` and submits batched.
    groups: Vec<(BlockState, u8, Vec<Cell>)>,
    /// Resolved `ModelRef` per group index, cleared on resource reload.
    models: Vec<Option<ModelRef>>,
    models_resolved: bool,
}

impl Variant {
    fn build(host: &dyn ClientHost, structure: &Structure, mirror: Mirror) -> Self {
        let offset = mirror_offset(structure.size, mirror);
        let facts = host.block_states();
        let mut palette_states: Vec<Option<(BlockState, bool)>> =
            Vec::with_capacity(structure.palette.len());
        for entry in &structure.palette {
            let mut props = entry.properties.clone();
            sch::transform_state(&entry.name, &mut props, mirror, Rotation::None);
            let resolved = resolve_client_state(host, &entry.name, &props).and_then(|s| {
                if is_air(host, s) {
                    None
                } else {
                    let solid = facts.is_solid_render(s) == Some(true);
                    Some((s, solid))
                }
            });
            palette_states.push(resolved);
        }

        let mut placed: Vec<(BlockState, [i32; 3])> = Vec::with_capacity(structure.blocks.len());
        let mut solid_cells: HashSet<[i32; 3]> = HashSet::new();
        for b in &structure.blocks {
            let Some(Some((state, solid))) = palette_states.get(b.state) else {
                continue;
            };
            let t = sch::transform(b.pos, mirror, Rotation::None, [0; 3]);
            let local = [t[0] + offset[0], t[1] + offset[1], t[2] + offset[2]];
            if *solid {
                solid_cells.insert(local);
            }
            placed.push((*state, local));
        }

        let mut by_state: HashMap<BlockState, Vec<([i32; 3], u8)>> = HashMap::new();
        let mut order: Vec<BlockState> = Vec::new();
        for (state, local) in placed {
            let hidden = hidden_face_mask(local, &solid_cells);
            if !by_state.contains_key(&state) {
                order.push(state);
            }
            by_state.entry(state).or_default().push((local, hidden));
        }

        let groups = order
            .into_iter()
            .filter_map(|s| {
                by_state.remove(&s).map(|list| {
                    let emission = facts.light_emission(s).unwrap_or(0).min(15);
                    (s, emission, list)
                })
            })
            .collect();
        Self {
            groups,
            models: Vec::new(),
            models_resolved: false,
        }
    }

    fn clear_models(&mut self) {
        self.models.clear();
        self.models_resolved = false;
    }

    fn resolve_models(&mut self, host: &dyn ClientHost) {
        if self.models_resolved {
            return;
        }
        let assets = host.render_assets();
        self.models = self
            .groups
            .iter()
            .map(|(state, _, _)| assets.block_state_model(*state).ok())
            .collect();
        self.models_resolved = true;
    }
}

/// The three prebuilt variants of the deployed schematic (`renderers[0..3]` in `SchematicHandler`).
pub(super) struct Preview {
    file: String,
    /// `[None, FrontBack, LeftRight]`.
    variants: [Variant; 3],
}

impl Preview {
    /// Load `file` from the client's `schematics/` folder and build all three mirror variants.
    pub(super) fn load(host: &dyn ClientHost, file: &str) -> Option<Self> {
        if !sch::valid_file(file) {
            return None;
        }
        let structure =
            super::body::read_structure(host, file).and_then(|tag| Structure::from_nbt(&tag))?;
        if structure.size == [0; 3] {
            return None;
        }
        let variants = [
            Variant::build(host, &structure, Mirror::None),
            Variant::build(host, &structure, Mirror::FrontBack),
            Variant::build(host, &structure, Mirror::LeftRight),
        ];
        Some(Self {
            file: file.to_owned(),
            variants,
        })
    }

    pub(super) fn matches(&self, file: &str) -> bool {
        self.file == file
    }

    pub(super) fn clear_models(&mut self) {
        for v in &mut self.variants {
            v.clear_models();
        }
    }

    /// `SchematicHandler.render` + `SchematicRenderer.render`: pick the active mirror variant and
    /// submit every block's model instance under `placing`.
    pub(super) fn render(
        &mut self,
        host: &dyn ClientHost,
        event: &mut RenderLevelStageEvent<'_>,
        placing: &Placing,
    ) {
        if event.stage != RenderLevelStage::AfterOpaqueBlocks {
            return;
        }
        let pt = event.view.partial_tick;
        let (fb, lr) = placing.scales(pt);
        let variant_idx = variant_index(fb, lr);
        let variant = &mut self.variants[variant_idx];
        variant.resolve_models(host);

        let (origin, linear) = placing.block_transform(pt);
        let level = host.level(host.dimension());
        let mut batch: Vec<ModelInstance> = Vec::new();

        'groups: for ((_, emission, cells), model) in
            variant.groups.iter().zip(variant.models.iter())
        {
            let Some(model) = *model else {
                continue;
            };
            let mut offset = 0;
            while offset < cells.len() {
                let usage = event.draw.usage();
                let remaining = usage
                    .instances_capacity
                    .saturating_sub(usage.instances_used) as usize;
                if remaining == 0 {
                    break 'groups;
                }
                let end = (offset + remaining).min(cells.len());
                batch.clear();
                batch.reserve(end - offset);
                for &(local, hidden_faces) in &cells[offset..end] {
                    let (world_origin, center) = instance_origin_and_center(origin, linear, local);
                    let light = sample_light(host, level, center, *emission);
                    batch.push(
                        ModelInstance::new(world_origin, linear, 0xFFFF_FFFF, light, Overlay::NONE)
                            .with_hidden_faces(hidden_faces),
                    );
                }
                if event.draw.draw_model_instances(model, &batch).is_err() {
                    break 'groups;
                }
                offset = end;
            }
        }
    }
}

fn instance_origin_and_center(
    origin: [f64; 3],
    linear: [f32; 12],
    local: [i32; 3],
) -> ([f64; 3], [f64; 3]) {
    let lx = f64::from(local[0]);
    let ly = f64::from(local[1]);
    let lz = f64::from(local[2]);
    let world_origin = [
        origin[0]
            + f64::from(linear[0]) * lx
            + f64::from(linear[1]) * ly
            + f64::from(linear[2]) * lz,
        origin[1]
            + f64::from(linear[4]) * lx
            + f64::from(linear[5]) * ly
            + f64::from(linear[6]) * lz,
        origin[2]
            + f64::from(linear[8]) * lx
            + f64::from(linear[9]) * ly
            + f64::from(linear[10]) * lz,
    ];
    let center = [
        world_origin[0]
            + 0.5 * (f64::from(linear[0]) + f64::from(linear[1]) + f64::from(linear[2])),
        world_origin[1]
            + 0.5 * (f64::from(linear[4]) + f64::from(linear[5]) + f64::from(linear[6])),
        world_origin[2]
            + 0.5 * (f64::from(linear[8]) + f64::from(linear[9]) + f64::from(linear[10])),
    ];
    (world_origin, center)
}

fn variant_index(fb: f32, lr: f32) -> usize {
    if lr < 0.0 && fb >= 0.0 {
        2
    } else if fb < 0.0 && lr >= 0.0 {
        1
    } else {
        0
    }
}

fn mirror_offset(size: [i32; 3], mirror: Mirror) -> [i32; 3] {
    match mirror {
        Mirror::None => [0, 0, 0],
        Mirror::FrontBack => [size[0].saturating_sub(1), 0, 0],
        Mirror::LeftRight => [0, 0, size[2].saturating_sub(1)],
    }
}

fn hidden_face_mask(local: [i32; 3], solid_cells: &HashSet<[i32; 3]>) -> u8 {
    let mut hidden = 0u8;
    for (bit, dir) in Direction::ALL.iter().enumerate() {
        let n = dir.normal();
        let adj = [local[0] + n[0], local[1] + n[1], local[2] + n[2]];
        if solid_cells.contains(&adj) {
            hidden |= 1u8 << bit;
        }
    }
    hidden
}

fn sample_light(
    host: &dyn ClientHost,
    level: Option<mod_api::LevelRef>,
    at: [f64; 3],
    emission: u8,
) -> LightCoords {
    let pos = at.map(|v| v.floor() as i32);
    level
        .and_then(|l| {
            let b = l.brightness(host, LightLayer::Block, pos)?;
            let s = l.brightness(host, LightLayer::Sky, pos)?;
            LightCoords::new((b.clamp(0, 15) as u8).max(emission), s.clamp(0, 15) as u8)
        })
        .unwrap_or(LightCoords::FULL_BRIGHT)
}

fn is_air(host: &dyn ClientHost, state: BlockState) -> bool {
    let name = state.block().name(host);
    matches!(
        name.as_str(),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "minecraft:structure_void"
    )
}

fn resolve_client_state(
    host: &dyn ClientHost,
    name: &str,
    props: &std::collections::BTreeMap<String, String>,
) -> Option<BlockState> {
    let default = host.block_state_id(name)?;
    if props.is_empty() {
        return Some(default);
    }
    let facts = host.block_states();
    let mut state = default;
    for (k, v) in props {
        if let Ok(next) = state.with(&facts, k, v) {
            state = next;
        }
    }
    Some(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrored_coordinates_stay_within_schematic_bounds() {
        let size = [3, 2, 5];
        for mirror in Mirror::ALL {
            let offset = mirror_offset(size, mirror);
            for x in 0..size[0] {
                for y in 0..size[1] {
                    for z in 0..size[2] {
                        let t = sch::transform([x, y, z], mirror, Rotation::None, [0; 3]);
                        let local = [t[0] + offset[0], t[1] + offset[1], t[2] + offset[2]];
                        for i in 0..3 {
                            assert!(
                                (0..size[i]).contains(&local[i]),
                                "{mirror:?} [{x}, {y}, {z}] -> {local:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn hidden_face_mask_culls_only_solid_neighbours() {
        let mut solid = HashSet::new();
        solid.insert([0, 0, 0]);
        solid.insert([1, 0, 0]); // east (bit 5)
        solid.insert([0, -1, 0]); // down (bit 0)
        let mask = hidden_face_mask([0, 0, 0], &solid);
        assert_eq!(mask, (1 << 0) | (1 << 5));
        assert_eq!(hidden_face_mask([1, 0, 0], &solid), 1 << 4); // west (bit 4)
    }

    #[test]
    fn variant_index_matches_schematic_handler() {
        assert_eq!(variant_index(1.0, 1.0), 0);
        assert_eq!(variant_index(-1.0, 1.0), 1);
        assert_eq!(variant_index(1.0, -1.0), 2);
        // Both negative is a 180-degree rotation of the unmirrored variant.
        assert_eq!(variant_index(-1.0, -1.0), 0);
    }
}
