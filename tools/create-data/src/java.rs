//! Facts read as text from Create's MIT Java registration chains (`REGISTRATE.block("name", ...)`
//! up to `.register()`): the block's class, its base properties and its sound. Nothing is run; a
//! chain built in a loop (palettes, colours) has no literal name and gets no facts.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct JavaFacts {
    /// `ShaftBlock`, from `ShaftBlock::new`, `ShaftBlock::small` or `new ShaftBlock(`.
    pub class: Option<String>,
    /// `stone`, `softMetal`, `wooden`, `copperMetal`, `netheriteMetal` (`SharedProperties::x`),
    /// or a vanilla block (`Blocks.GLASS` gives `glass`).
    pub base: Option<String>,
    /// `SoundType.X`, lowercased: `metal`, `wood`, `netherite_block`... The last one the chain
    /// sets, counting a `BuilderTransformers` helper's where `.transform` calls it, as Registrate
    /// applies `properties` in chain order.
    pub sound: Option<String>,
    /// `lightLevel($ -> n)`: a light level that is a number, whatever the state.
    pub light: Option<u8>,
    /// `.tag(Tags.Blocks.RELOCATION_NOT_SUPPORTED)` (`c:relocation_not_supported`): a block nothing
    /// should move.
    pub relocation_not_supported: bool,
    pub no_occlusion: bool,
    /// `.requiresCorrectToolForDrops()` set in the chain itself.
    pub requires_tool: bool,
    /// A `BuilderTransformers` helper in the chain, which may set base properties of its own.
    pub transformed: bool,
    /// The base properties a `BuilderTransformers` helper in the chain starts from.
    pub transformer_base: Option<String>,
    /// An item chain that sets `.food(...)`.
    pub food: bool,
    /// `CStress.setImpact(x)` (`setNoImpact` is 0), directly or through a `BuilderTransformers`
    /// helper that sets it: stress units per RPM.
    pub impact: Option<f64>,
    /// `CStress.setCapacity(x)`: stress units per RPM of generated speed.
    pub capacity: Option<f64>,
}

/// `BuilderTransformers` helpers that set a stress impact (BuilderTransformers.java).
const TRANSFORMER_IMPACTS: [(&str, f64); 3] = [
    ("BuilderTransformers.cuckooClock(", 1.0),
    ("BuilderTransformers.mechanicalPiston(", 4.0),
    ("BuilderTransformers.backtank(", 4.0),
];

/// `BuilderTransformers` helpers that start the chain from base properties
/// (`initialProperties(SharedProperties::..)`, BuilderTransformers.java).
const TRANSFORMER_BASES: [(&str, &str); 17] = [
    ("BuilderTransformers.encasedShaft(", "stone"),
    ("BuilderTransformers.encasedCogwheel(", "stone"),
    ("BuilderTransformers.encasedLargeCogwheel(", "stone"),
    ("BuilderTransformers.casing(", "stone"),
    ("BuilderTransformers.layeredCasing(", "stone"),
    ("BuilderTransformers.beltTunnel(", "stone"),
    ("BuilderTransformers.mechanicalPiston(", "stone"),
    ("BuilderTransformers.bearing(", "stone"),
    ("BuilderTransformers.crate(", "stone"),
    ("BuilderTransformers.valveHandle(", "copperMetal"),
    ("BuilderTransformers.bogey(", "softMetal"),
    ("BuilderTransformers.copycat(", "softMetal"),
    ("BuilderTransformers.bell(", "softMetal"),
    ("BuilderTransformers.packager(", "softMetal"),
    ("BuilderTransformers.palettesIronBlock(", "softMetal"),
    ("BuilderTransformers.cuckooClock(", "wooden"),
    ("BuilderTransformers.slidingDoor(", "iron_door"),
];

fn number_after(s: &str, prefix: &str) -> Option<f64> {
    let i = s.find(prefix)? + prefix.len();
    let t = &s[i..];
    let n = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    t[..n].parse().ok()
}

/// Item registrations: `REGISTRATE.item("name", X::new)` chains, and Create's `ingredient("name")`
/// and `taggedIngredient("name", ...)` helpers, which register plain `Item`s.
pub fn read_items(java_root: &Path) -> BTreeMap<String, JavaFacts> {
    let mut out = BTreeMap::new();
    let mut files = Vec::new();
    collect(java_root, &mut files);
    files.sort();
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for (name, mut facts) in chains(&text, ".item(\"") {
            facts.food = facts.food || text_has_food(&text, &name);
            out.entry(name).or_insert(facts);
        }
        for (marker, class) in [
            ("= ingredient(\"", "Item"),
            (" ingredient(\"", "Item"),
            ("taggedIngredient(\"", "Item"),
            ("sequencedIngredient(\"", "SequencedAssemblyItem"),
        ] {
            let mut rest = text.as_str();
            while let Some(at) = rest.find(marker) {
                let after = &rest[at + marker.len()..];
                let Some(end) = after.find('"') else { break };
                out.entry(after[..end].to_string()).or_insert(JavaFacts {
                    class: Some(class.into()),
                    ..JavaFacts::default()
                });
                rest = &after[end..];
            }
        }
    }
    out
}

fn text_has_food(text: &str, name: &str) -> bool {
    let Some(at) = text.find(&format!(".item(\"{name}\"")) else {
        return false;
    };
    let chain = &text[at..];
    let end = chain.find(".register()").unwrap_or(chain.len());
    chain[..end].contains(".food(")
}

pub fn read_all(java_root: &Path) -> BTreeMap<String, JavaFacts> {
    let mut out = BTreeMap::new();
    let mut files = Vec::new();
    collect(java_root, &mut files);
    files.sort();
    let helpers = fs::read_to_string(
        java_root.join("com/simibubi/create/foundation/data/BuilderTransformers.java"),
    )
    .map(|text| transformer_sounds(&text))
    .unwrap_or_default();
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for (name, facts) in chains_with(&text, ".block(\"", &helpers) {
            out.entry(name).or_insert(facts);
        }
    }
    out
}

/// The sound each `BuilderTransformers` helper sets (`p.sound(SoundType.X)` in its body), as
/// (`BuilderTransformers.name(`, `x`): `casing` gives wood, `bogey` netherite.
pub fn transformer_sounds(text: &str) -> Vec<(String, String)> {
    let mut starts: Vec<usize> = ["public static ", "private static ", "protected static "]
        .iter()
        .flat_map(|m| text.match_indices(m).map(|(i, _)| i))
        .collect();
    starts.sort_unstable();
    let mut out = Vec::new();
    for (k, &at) in starts.iter().enumerate() {
        let body = &text[at..starts.get(k + 1).copied().unwrap_or(text.len())];
        let Some(open) = body.find('(') else { continue };
        let name = body[..open]
            .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        if let Some(sound) = ident_after(body, "SoundType.").filter(|_| !name.is_empty()) {
            out.push((
                format!("BuilderTransformers.{name}("),
                sound.to_ascii_lowercase(),
            ));
        }
    }
    out
}

/// The blocks of `names` that have a block entity (`EntityBlock`): every block an
/// `AllBlockEntityTypes` registration names (`validBlock(s)`, a dyed family's `toArray()`, the
/// tracks of `validBlocksDeferred`), its `AllBlocks` constants resolved to their block names.
pub fn block_entity_blocks(
    java_root: &Path,
    names: &[String],
) -> std::collections::BTreeSet<String> {
    let base = java_root.join("com/simibubi/create");
    let blocks = fs::read_to_string(base.join("AllBlocks.java")).unwrap_or_default();
    let types = fs::read_to_string(base.join("AllBlockEntityTypes.java")).unwrap_or_default();
    // Each constant's block names, from its declaration to the next one.
    let mut constants: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let decls: Vec<usize> = blocks
        .match_indices("public static final ")
        .map(|(i, _)| i)
        .collect();
    for (k, &at) in decls.iter().enumerate() {
        let body = &blocks[at..decls.get(k + 1).copied().unwrap_or(blocks.len())];
        let Some(eq) = body.find(" =") else { continue };
        let Some(constant) = body[..eq].rsplit(' ').next() else {
            continue;
        };
        let found = if body.contains("DyedBlockList<") {
            // `colourName + "_toolbox"`: every dyed block of that family, and a block the
            // family returns for a colour (`return ORANGE_NIXIE_TUBE`).
            let mut v: Vec<String> = ident_after(body, "colourName + \"_")
                .map(|family| {
                    names
                        .iter()
                        .filter(|n| {
                            n.strip_suffix(family)
                                .and_then(|c| c.strip_suffix('_'))
                                .is_some_and(|c| crate::blocks::DYES.contains(&c))
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            if let Some(other) = ident_after(body, "return ")
                .filter(|c| c.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
            {
                v.push(format!("={other}"));
            }
            v
        } else {
            body.find(".block(\"")
                .map(|i| {
                    let rest = &body[i + 8..];
                    vec![rest[..rest.find('"').unwrap_or(0)].to_string()]
                })
                .unwrap_or_default()
        };
        constants.insert(constant.to_string(), found);
    }
    let resolve = |c: &str| -> Vec<String> {
        let mut out = Vec::new();
        for n in constants.get(c).into_iter().flatten() {
            match n.strip_prefix('=') {
                Some(other) => out.extend(constants.get(other).into_iter().flatten().cloned()),
                None => out.push(n.clone()),
            }
        }
        out
    };
    let mut out = std::collections::BTreeSet::new();
    for chain in types.split(".blockEntity(\"").skip(1) {
        let chain = &chain[..chain.find(".register()").unwrap_or(chain.len())];
        let mut rest = chain;
        while let Some(i) = rest.find("AllBlocks.") {
            let c = ident_after(&rest[i..], "AllBlocks.").unwrap_or("");
            out.extend(resolve(c));
            rest = &rest[i + 10..];
        }
        if chain.contains("validBlocksDeferred(TrackMaterial::allBlocks)") {
            out.insert("track".to_string());
        }
    }
    out.retain(|n| names.contains(n));
    out
}

pub fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "java") {
            out.push(p);
        }
    }
}

/// Every `.block("literal", ...)` chain in one file.
pub fn chains(text: &str, marker: &str) -> Vec<(String, JavaFacts)> {
    chains_with(text, marker, &[])
}

/// [`chains`], with the sounds `BuilderTransformers` helpers set ([`transformer_sounds`]).
pub fn chains_with(
    text: &str,
    marker: &str,
    helpers: &[(String, String)],
) -> Vec<(String, JavaFacts)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(marker) {
        let after = &rest[at + marker.len()..];
        let Some(end_name) = after.find('"') else {
            break;
        };
        let name = &after[..end_name];
        let body_end = after.find(".register()").unwrap_or(after.len());
        let body = &after[end_name..body_end];
        if name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            out.push((name.to_string(), facts(body, helpers)));
        }
        rest = &after[end_name..];
    }
    out
}

fn ident_after<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let i = s.find(prefix)? + prefix.len();
    let t = &s[i..];
    let n = t
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(t.len());
    (n > 0).then(|| &t[..n])
}

/// The sound the chain ends with: its own `SoundType.X`s and its helpers' ([`transformer_sounds`])
/// in chain order, the last winning (a copper casing's own copper gives way to `casing`'s wood).
fn last_sound(body: &str, helpers: &[(String, String)]) -> Option<String> {
    let own = body.match_indices("SoundType.").filter_map(|(i, _)| {
        Some((
            i,
            ident_after(&body[i..], "SoundType.")?.to_ascii_lowercase(),
        ))
    });
    let by_helpers = helpers.iter().flat_map(|(call, sound)| {
        body.match_indices(call.as_str())
            .map(move |(i, _)| (i, sound.clone()))
    });
    own.chain(by_helpers)
        .max_by_key(|(i, _)| *i)
        .map(|(_, sound)| sound)
}

/// `lightLevel(s -> n)` with `n` a number (`$ -> 12`); a light by state (`s -> ... ? 15 : 0`, a
/// method reference) is none.
fn constant_light(body: &str) -> Option<u8> {
    let at = body.find(".lightLevel(")? + ".lightLevel(".len();
    let inner = &body[at..at + body[at..].find(')')?];
    let (_, n) = inner.split_once("->")?;
    n.trim().parse().ok().filter(|n| *n <= 15)
}

fn facts(body: &str, helpers: &[(String, String)]) -> JavaFacts {
    // The constructor is the second argument of `.block(name, X::new)` or a lambda `new X(`.
    let head = &body[..body
        .find(')')
        .map(|i| i + 1)
        .unwrap_or(body.len())
        .min(body.len())];
    // A lambda `p -> new X(p, ...)` names the class; otherwise the `X::new` reference does.
    let class = ident_after(head, "new ").map(str::to_string).or_else(|| {
        head.split([',', ' ', '(', '>']).find_map(|tok| {
            tok.trim()
                .split_once("::")
                .map(|(c, _)| c.to_string())
                .filter(|c| c.chars().next().is_some_and(char::is_uppercase))
        })
    });
    let base = ident_after(body, "initialProperties(SharedProperties::")
        .map(str::to_string)
        .or_else(|| {
            ident_after(body, "initialProperties(() -> Blocks.").map(|s| s.to_ascii_lowercase())
        });
    let transformer_base = TRANSFORMER_BASES
        .iter()
        .find(|(t, _)| body.contains(t))
        .map(|(_, b)| (*b).to_string());
    JavaFacts {
        class,
        base,
        transformer_base,
        sound: last_sound(body, helpers),
        light: constant_light(body),
        relocation_not_supported: body.contains("Tags.Blocks.RELOCATION_NOT_SUPPORTED"),
        // `BuilderTransformers.encasedBase` (under the encased shafts and cogwheels), `bearing`
        // and `mechanicalPiston` set it in their own chains.
        no_occlusion: body.contains("noOcclusion()")
            || body.contains("Properties::noOcclusion")
            || [
                "BuilderTransformers.encasedShaft(",
                "BuilderTransformers.encasedCogwheel(",
                "BuilderTransformers.encasedLargeCogwheel(",
                "BuilderTransformers.bearing(",
                "BuilderTransformers.mechanicalPiston(",
            ]
            .iter()
            .any(|t| body.contains(t)),
        requires_tool: body.contains("requiresCorrectToolForDrops()"),
        transformed: body.contains("BuilderTransformers."),
        food: body.contains(".food("),
        impact: number_after(body, "CStress.setImpact(")
            .or_else(|| body.contains("CStress.setNoImpact()").then_some(0.0))
            .or_else(|| {
                TRANSFORMER_IMPACTS
                    .iter()
                    .find(|(t, _)| body.contains(t))
                    .map(|(_, v)| *v)
            }),
        capacity: number_after(body, "CStress.setCapacity("),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_registration_chain() {
        let java = r#"
	public static final BlockEntry<CogWheelBlock> COGWHEEL = REGISTRATE.block("cogwheel", CogWheelBlock::small)
		.initialProperties(SharedProperties::stone)
		.properties(p -> p.sound(SoundType.WOOD)
			.mapColor(MapColor.DIRT))
		.transform(CStress.setNoImpact())
		.register();
	public static final BlockEntry<CreativeMotorBlock> CREATIVE_MOTOR = REGISTRATE.block("creative_motor", CreativeMotorBlock::new)
		.transform(CStress.setCapacity(16384.0))
		.register();
	public static final BlockEntry<EncasedShaftBlock> X =
		REGISTRATE.block("andesite_encased_shaft", p -> new EncasedShaftBlock(p, AllBlocks.ANDESITE_CASING::get))
			.register();
"#;
        let c = chains(java, ".block(\"");
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].1.impact, Some(0.0));
        assert_eq!(c[1].1.capacity, Some(16384.0));
        assert_eq!(c[0].0, "cogwheel");
        assert_eq!(c[0].1.class.as_deref(), Some("CogWheelBlock"));
        assert_eq!(c[0].1.base.as_deref(), Some("stone"));
        assert_eq!(c[0].1.sound.as_deref(), Some("wood"));
        assert_eq!(c[2].1.class.as_deref(), Some("EncasedShaftBlock"));
    }

    #[test]
    fn the_last_sound_in_chain_order_wins_with_its_helpers() {
        let transformers = r#"
	public static <B extends CasingBlock> NonNullUnaryOperator<BlockBuilder<B, CreateRegistrate>> casing(
		Supplier<CTSpriteShiftEntry> ct) {
		return b -> b.initialProperties(SharedProperties::stone)
			.properties(p -> p.sound(SoundType.WOOD))
			.build();
	}

	private static <B extends Block, P> BlockBuilder<B, P> plain(BlockBuilder<B, P> b) {
		return b.initialProperties(SharedProperties::stone);
	}

	public static <B extends CasingBlock> NonNullUnaryOperator<BlockBuilder<B, CreateRegistrate>> layeredCasing(
		Supplier<CTSpriteShiftEntry> ct, Supplier<CTSpriteShiftEntry> ct2) {
		return b -> b.initialProperties(SharedProperties::stone);
	}
"#;
        let helpers = transformer_sounds(transformers);
        assert_eq!(
            helpers,
            [(
                "BuilderTransformers.casing(".to_string(),
                "wood".to_string()
            )]
        );
        let java = r#"
	public static final BlockEntry<CasingBlock> COPPER_CASING = REGISTRATE.block("copper_casing", CasingBlock::new)
		.properties(p -> p.mapColor(MapColor.TERRACOTTA_LIGHT_GRAY)
			.sound(SoundType.COPPER))
		.transform(BuilderTransformers.casing(() -> AllSpriteShifts.COPPER_CASING))
		.register();
	public static final BlockEntry<CasingBlock> RAILWAY_CASING = REGISTRATE.block("railway_casing", CasingBlock::new)
		.transform(BuilderTransformers.layeredCasing(() -> AllSpriteShifts.RAILWAY_CASING_SIDE,
			() -> AllSpriteShifts.RAILWAY_CASING))
		.properties(p -> p.sound(SoundType.NETHERITE_BLOCK))
		.register();
	public static final BlockEntry<CasingBlock> X = REGISTRATE.block("loud_casing", CasingBlock::new)
		.transform(BuilderTransformers.casing(() -> AllSpriteShifts.ANDESITE_CASING))
		.properties(p -> p.sound(SoundType.ANVIL))
		.register();
"#;
        let c = chains_with(java, ".block(\"", &helpers);
        // `casing` sets wood after the copper casing's own copper.
        assert_eq!(c[0].1.sound.as_deref(), Some("wood"));
        // A helper without a sound leaves the chain's.
        assert_eq!(c[1].1.sound.as_deref(), Some("netherite_block"));
        // The chain's own sound after the helper's wins.
        assert_eq!(c[2].1.sound.as_deref(), Some("anvil"));
        // Without the helpers, the chain's own sounds only.
        assert_eq!(
            chains(java, ".block(\"")[0].1.sound.as_deref(),
            Some("copper")
        );
    }

    #[test]
    fn reads_a_constant_light_and_the_relocation_tag() {
        let java = r#"
	public static final BlockEntry<CasingBlock> A = REGISTRATE.block("refined_radiance_casing", CasingBlock::new)
		.properties(p -> p.lightLevel($ -> 12))
		.register();
	public static final BlockEntry<ExperienceBlock> B = REGISTRATE.block("experience_block", ExperienceBlock::new)
		.properties(p -> p.requiresCorrectToolForDrops()
			.lightLevel(s -> 15))
		.blockstate((c, p) -> p.simpleBlock(c.get()))
		.register();
	public static final BlockEntry<Block> C = REGISTRATE.block("rose_quartz_lamp", Block::new)
		.properties(p -> p.lightLevel(s -> s.getValue(RoseQuartzLampBlock.POWERING) ? 15 : 0))
		.register();
	public static final BlockEntry<Block> D = REGISTRATE.block("lit_blaze_burner", Block::new)
		.properties(p -> p.lightLevel(LitBlazeBurnerBlock::getLight))
		.blockstate((c, p) -> p.simpleBlock(c.get()))
		.register();
	public static final BlockEntry<TrackBlock> E = REGISTRATE.block("track", TrackMaterial.ANDESITE::createBlock)
		.tag(Tags.Blocks.RELOCATION_NOT_SUPPORTED)
		.register();
"#;
        let c = chains(java, ".block(\"");
        let light: Vec<Option<u8>> = c.iter().map(|(_, f)| f.light).collect();
        assert_eq!(light, [Some(12), Some(15), None, None, None]);
        let tagged: Vec<bool> = c.iter().map(|(_, f)| f.relocation_not_supported).collect();
        assert_eq!(tagged, [false, false, false, false, true]);
    }
}
