//! Wrench, potato cannon and worldshaper item models. Their original jar parts draw under the
//! shell's display transform; only the moving part takes Create's rotation about its pivot.

use crate::glue::pipelines::Pose;
use create_engine::equipment::ToolSpin;
use mod_api::{
    ClientHost, ItemStack, SpecialModelPart, SpecialModelRenderer, SpecialModelRenderers, Tag,
};
use std::sync::Arc;

#[derive(Clone, Copy)]
struct Spinner {
    spin: ToolSpin,
    fixed: &'static [&'static str],
    moving: &'static str,
}

/// Horizon asks a special model once per changed stack, so the moving part is posed where it
/// was then rather than turning every frame.
struct Spinning(Spinner);

pub(super) fn register(renderers: &mut dyn SpecialModelRenderers) {
    for (id, renderer) in [
        (
            "create:wrench",
            Spinner {
                spin: ToolSpin::Wrench,
                fixed: &["create:item/wrench/item"],
                moving: "create:item/wrench/gear",
            },
        ),
        (
            "create:potato_cannon",
            Spinner {
                spin: ToolSpin::PotatoCannon,
                fixed: &["create:item/potato_cannon/item"],
                moving: "create:item/potato_cannon/cog",
            },
        ),
        (
            "create:handheld_worldshaper",
            Spinner {
                spin: ToolSpin::Worldshaper,
                fixed: &[
                    "create:item/handheld_worldshaper/item",
                    "create:item/handheld_worldshaper/core",
                    "create:item/handheld_worldshaper/core_glow",
                ],
                moving: "create:item/handheld_worldshaper/accelerator",
            },
        ),
    ] {
        renderers.register(id.to_owned(), Arc::new(Spinning(renderer)));
    }
}

impl SpecialModelRenderer for Spinning {
    fn extract(
        &self,
        host: &dyn ClientHost,
        _stack: &ItemStack,
        _config: &Tag,
    ) -> Result<Vec<SpecialModelPart>, String> {
        let seconds = host.millis() as f64 / 1000.0;
        let mut parts: Vec<SpecialModelPart> =
            self.0.fixed.iter().map(|m| SpecialModelPart::new(*m)).collect();
        let (degrees, axis, pivot) = self.0.spin.pose(seconds);
        let pose = Pose::at([0.0; 3]).rotate(axis, degrees, pivot);
        parts.push(SpecialModelPart::new(self.0.moving).with_transform(pose.affine()));
        Ok(parts)
    }
}
