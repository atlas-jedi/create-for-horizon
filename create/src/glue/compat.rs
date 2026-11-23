//! Small adapters from Horizon's hook shapes to the subject/view shapes the glue was written
//! against (the host and a read-only view of the block entity, reached through `subject`).

use mod_api::{BlockEntityClientTickEvent, BlockEntityRef, BlockState, ClientHost, Compound, LevelRef};

/// What a client-ticking block entity looks like.
pub struct BeView<'a> {
    state: BlockState,
    pos: [i32; 3],
    level: Option<LevelRef>,
    nbt: &'a Compound,
    handle: BlockEntityRef,
}

impl BeView<'_> {
    pub fn state(&self) -> BlockState {
        self.state
    }
    pub fn position(&self) -> [i32; 3] {
        self.pos
    }
    pub fn level(&self) -> LevelRef {
        self.level.expect("checked in with_view")
    }
    pub fn nbt(&self) -> &Compound {
        self.nbt
    }
    #[allow(dead_code)]
    pub fn handle(&self) -> BlockEntityRef {
        self.handle
    }
}

/// The block entity being ticked, and the host to reach the game with.
pub struct ClientBeSubject<'a> {
    host: &'a mut dyn ClientHost,
    view: BeView<'a>,
}

impl<'a> ClientBeSubject<'a> {
    pub fn with_view<R>(&self, f: impl FnOnce(&BeView<'a>) -> R) -> Option<R> {
        self.view.level?;
        Some(f(&self.view))
    }
    pub fn host(&mut self) -> &mut dyn ClientHost {
        &mut *self.host
    }
}

/// `block_entity_client_tick`'s event as a subject.
pub struct ClientTick<'a> {
    pub subject: ClientBeSubject<'a>,
}

impl<'a> ClientTick<'a> {
    pub fn new(host: &'a mut dyn ClientHost, event: &'a BlockEntityClientTickEvent<'a>) -> Self {
        let level = event.block_entity.level(&*host);
        ClientTick {
            subject: ClientBeSubject {
                host,
                view: BeView {
                    state: event.state,
                    pos: event.pos,
                    level,
                    nbt: &event.entity.nbt,
                    handle: event.block_entity,
                },
            },
        }
    }
}

/// Sunrise's `SetPlacedByEvent` (a block part's), rebuilt from Horizon's item-part
/// `on_block_placed_by`. The level is the placer's: a block placed without a player has none.
pub struct SetPlacedByEvent<'a> {
    pub stack: Option<&'a mod_api::ItemStack>,
    pub level: PlacedLevel,
    pub pos: [i32; 3],
}

/// The placed block's level, by its dimension's name.
pub struct PlacedLevel(Option<String>);

impl PlacedLevel {
    pub fn dimension(&self) -> &Self {
        self
    }
    pub fn id<H: ?Sized>(&self, _host: &H) -> Option<String> {
        self.0.clone()
    }
}

impl<'a> SetPlacedByEvent<'a> {
    pub fn new(host: &dyn mod_api::ServerHost, event: &mod_api::OnBlockPlacedByEvent<'a>) -> Self {
        let dimension = event
            .placer
            .and_then(|p| p.dimension(host))
            .map(|d| d.to_string());
        SetPlacedByEvent {
            stack: Some(event.stack),
            level: PlacedLevel(dimension),
            pos: event.pos,
        }
    }
}

/// Sunrise's creative catalog, which Horizon doesn't read back: always empty here, so the
/// attribute filter's "in item group" never matches.
pub struct CreativeModeTabs {
    pub tabs: Vec<CreativeModeTabInfo>,
}

pub struct CreativeModeTabInfo {
    pub id: String,
    pub category: bool,
    pub display_name: mod_api::Text,
}

impl CreativeModeTabs {
    pub fn tab(&self, id: &str) -> Option<&CreativeModeTabInfo> {
        self.tabs.iter().find(|t| t.id == id)
    }
}

impl CreativeModeTabInfo {
    pub fn contains(&self, _host: &dyn mod_api::Host, _stack: &mod_api::ItemStack) -> bool {
        false
    }
}

/// Sunrise's typed particle: Horizon names custom particles by a provider key, so this is the
/// key and the name the provider is registered under. Options carry nothing.
#[derive(Clone, Copy)]
pub struct ParticleType<T = ()> {
    key: mod_api::ParticleKey,
    name: &'static str,
    _options: std::marker::PhantomData<fn() -> T>,
}

impl ParticleType<()> {
    pub fn new(key: mod_api::ParticleKey, name: &'static str) -> Self {
        ParticleType {
            key,
            name,
            _options: std::marker::PhantomData,
        }
    }
    pub fn options(&self, _: ()) -> mod_api::ParticleOptions {
        mod_api::ParticleOptions::simple(self.key)
    }
}

/// What a provider's tick sees of the level: Horizon gives it nothing, so it reads as an
/// unlit, empty level.
pub trait ParticleLevelView {
    fn brightness(&self, _layer: mod_api::LightLayer, _at: [i32; 3]) -> i32 {
        15
    }
}

struct NoLevel;
impl ParticleLevelView for NoLevel {}

/// Sunrise's batch of particles of one type: here always the one Horizon ticks.
pub struct ParticleBatch<'a>(&'a mut mod_api::ModParticle);

impl<'a> ParticleBatch<'a> {
    pub fn iter_mut(&mut self) -> std::iter::Once<&mut mod_api::ModParticle> {
        std::iter::once(&mut *self.0)
    }
}

/// Sunrise's batch particle provider.
pub trait ParticleProvider: Send + Sync + 'static {
    fn tick(&self, particles: &mut ParticleBatch<'_>, level: &dyn ParticleLevelView);
}

struct Provider<P>(P);

impl<P: ParticleProvider> mod_api::ParticleProvider for Provider<P> {
    fn tick(&self, particle: &mut mod_api::ModParticle) {
        self.0.tick(&mut ParticleBatch(particle), &NoLevel);
    }
}

/// `register_sprite_set(type, provider)` on Horizon's by-name registry.
pub fn register_sprite_set<P: ParticleProvider>(
    providers: &mut dyn mod_api::ParticleProviders,
    ty: ParticleType<()>,
    provider: P,
) {
    providers.register(ty.name.to_owned(), std::sync::Arc::new(Provider(provider)));
}

/// The level a block was placed in, for Horizon's `on_block_placed_by` (Sunrise's event
/// carried it): the placer's. None for a block placed without a player.
pub fn placed_level(
    host: &dyn mod_api::Host,
    event: &mod_api::OnBlockPlacedByEvent<'_>,
) -> Option<LevelRef> {
    event.placer?.level(host)
}

/// Sunrise's `Host::text_string` on either side; Horizon formats text only on a client, so the
/// server resolves the translation itself (the language file's `%s`/`%n$s` placeholders).
pub fn text_string(host: &dyn mod_api::Host, text: &mod_api::Text) -> String {
    use mod_api::text::TextContent;
    let head = match &text.content {
        TextContent::Translatable {
            key,
            fallback,
            arguments,
        } => {
            let _ = host;
            let mut s = fallback.clone().unwrap_or_else(|| key.clone());
            for (i, a) in arguments.iter().enumerate() {
                let a = text_string(host, a);
                s = s.replace(&format!("%{}$s", i + 1), &a);
                s = s.replacen("%s", &a, 1);
            }
            s
        }
        TextContent::Literal(s) => s.clone(),
        _ => String::new(),
    };
    head + &text
        .children
        .iter()
        .map(|c| text_string(host, c))
        .collect::<String>()
}

/// What a spawned entity starts with (Sunrise's `EntitySpawn`). Horizon spawns a mod's general
/// entity through `/summon`'s compound: the velocity goes as its `Motion`; an owner isn't kept.
pub struct EntitySpawn {
    pub owner: Option<mod_api::EntityRef>,
    pub velocity: [f64; 3],
}

pub enum SpawnBody {
    General,
}

impl EntitySpawn {
    pub fn new(_body: SpawnBody) -> Self {
        EntitySpawn {
            owner: None,
            velocity: [0.0; 3],
        }
    }
}

/// `LevelRef::spawn_entity` over Horizon's `summon`.
pub trait LevelSpawn {
    fn spawn_entity(
        self,
        host: &mut dyn mod_api::ServerHost,
        kind: &str,
        pos: [f64; 3],
        spawn: &EntitySpawn,
    ) -> Option<mod_api::EntityRef>;
}

impl LevelSpawn for LevelRef {
    fn spawn_entity(
        self,
        host: &mut dyn mod_api::ServerHost,
        kind: &str,
        pos: [f64; 3],
        spawn: &EntitySpawn,
    ) -> Option<mod_api::EntityRef> {
        use mod_api::Tag;
        let dimension = self.dimension().id(&*host)?;
        let mut nbt = Compound::new();
        if spawn.velocity != [0.0; 3] {
            nbt.insert(
                "Motion".into(),
                Tag::List(spawn.velocity.iter().map(|&v| Tag::Double(v.to_bits())).collect()),
            );
        }
        let id = host.summon(&dimension, kind, pos, nbt)?;
        host.entity(id)
    }
}

/// Sunrise's `hover_name` as text; Horizon gives the plain string.
pub fn hover_text<H: mod_api::ClientHost + ?Sized>(host: &H, stack: &mod_api::ItemStack) -> mod_api::Text {
    mod_api::Text::literal(host.hover_name(stack))
}

/// Whether an item's model is lit as a block's (`usesBlockLight`): Horizon doesn't say, so an
/// item that places a block stands for it.
pub fn item_uses_block_light<H: mod_api::ClientHost + ?Sized>(
    host: &H,
    stack: &mod_api::ItemStack,
    _context: mod_api::ItemDisplayContext,
) -> bool {
    host.item_block(stack.item()).is_some()
}

/// Sunrise's offer a block entity capability provider answers into; Horizon's provider returns
/// the handler instead.
pub struct BlockCapabilityOffer<'a, T: ?Sized>(&'a mut Option<Box<T>>);

impl<T: ?Sized> BlockCapabilityOffer<'_, T> {
    pub fn offer(&mut self, handler: impl FnOnce() -> Box<T>) {
        if self.0.is_none() {
            *self.0 = Some(handler());
        }
    }
}

/// A level host that knows nothing: Horizon's providers get the game host, not a level view,
/// and Create's providers don't read it.
struct NoLevelHost;
impl mod_api::LevelHost for NoLevelHost {}

/// A Sunrise-shaped block entity capability provider as Horizon's.
pub fn be_offer<T: ?Sized + 'static>(
    provider: fn(
        &dyn mod_api::LevelHost,
        mod_api::BlockSide,
        BlockState,
        BlockEntityRef,
        &mut BlockCapabilityOffer<'_, T>,
    ),
) -> impl Fn(&mut dyn mod_api::ServerHost, mod_api::BlockSide) -> Option<Box<T>> + Send + Sync + 'static
{
    move |host, side| {
        let state = side.level.block_state(&*host, side.pos)?;
        let entity = host.block_entity(side.level, side.pos)?;
        let mut out = None;
        provider(&NoLevelHost, side, state, entity, &mut BlockCapabilityOffer(&mut out));
        out
    }
}

/// Whether the block at `pos` has a collision shape (Sunrise's `collision_shape` not empty).
/// Horizon's level view has no shapes: anything but air and plain fluids counts.
pub fn has_collision(level: &dyn mod_api::LevelHost, pos: [i32; 3]) -> Option<bool> {
    let state = level.block_state(pos)?;
    let name = level.state_block(state)?;
    Some(!matches!(
        name,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "minecraft:water" | "minecraft:lava"
    ))
}

/// Sunrise's vanilla block classes (`instanceof`), which Horizon doesn't answer: read from the
/// block's id instead (`oak_stairs` is a stair).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockClass {
    Entity,
    Stair,
    IronBars,
    StainedGlassPane,
    TrapDoor,
}

pub fn block_class(host: &dyn mod_api::Host, state: BlockState, class: BlockClass) -> Option<bool> {
    use mod_api::HostBlockStates as _;
    let id = state.block().id(host);
    let path = id.split_once(':').map_or(id.as_str(), |(_, p)| p);
    Some(match class {
        BlockClass::Entity => host.block_states().has_block_entity(state)?,
        BlockClass::Stair => path.ends_with("_stairs"),
        BlockClass::IronBars => path == "iron_bars" || path.ends_with("glass_pane") || path == "glass_pane",
        BlockClass::StainedGlassPane => path.ends_with("stained_glass_pane"),
        BlockClass::TrapDoor => path.ends_with("_trapdoor"),
    })
}

/// Sunrise's copper weathering age. Horizon's level view doesn't tell a state's age, so no
/// state has one: Create's copper doesn't age by itself.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum WeatheringAge {
    Unaffected,
    Exposed,
    Weathered,
    Oxidized,
}

pub fn weathering_age(_state: BlockState) -> Option<WeatheringAge> {
    None
}

/// Sunrise host methods Horizon doesn't have, as stubs that do nothing (or answer "unknown"),
/// so the code around them stays as it was. Each one is listed in `HORIZON_GAPS.md`.
pub mod ext {
    use mod_api::{
        BlockEntityRef, BlockState, ClientHost, EntityRef, Host, ItemStack, LevelRef, PlayerRef,
        PushReaction, ServerHost, SoundRef, Tag,
    };

    /// A contraption's collision boxes (Sunrise's entity colliders).
    #[derive(Clone, Debug)]
    pub struct Collider {
        pub boxes: Vec<ColliderBox>,
        pub rotation: [f32; 4],
    }

    #[derive(Clone, Copy, Debug)]
    pub struct ColliderBox {
        pub min: [f64; 3],
        pub max: [f64; 3],
    }

    /// A block entity's own shapes (Sunrise's `BlockEntityShapes`).
    pub struct BlockEntityShapes {
        pub collision: Vec<[f64; 6]>,
        pub outline: Vec<[f64; 6]>,
    }

    pub trait EntityExt: Sized {
        fn set_client_fire_immunity(self, _host: &mut dyn ClientHost, _on: bool) -> bool {
            false
        }
        fn set_fire_immunity(self, _host: &mut dyn ServerHost, _on: bool) -> bool {
            false
        }
        fn set_sprinting(self, _host: &mut dyn ServerHost, _on: bool) -> bool {
            false
        }
        fn piston_push_reaction(self, _host: &dyn Host) -> Result<Option<PushReaction>, ()> {
            Ok(None)
        }
        fn is_hanging(self, _host: &dyn Host) -> Result<Option<bool>, ()> {
            Ok(None)
        }
        fn owner(self, _host: &dyn Host) -> Option<EntityRef> {
            None
        }
        fn set_collider(self, _host: &mut dyn ServerHost, _collider: Option<Collider>) -> Result<(), ()> {
            Ok(())
        }
        fn collider(self, _host: &dyn Host) -> Option<Collider> {
            None
        }
        fn set_collider_transform<R>(self, _host: &mut dyn ServerHost, _rotation: R) -> Result<(), ()> {
            Ok(())
        }
        fn move_by(
            self,
            host: &mut dyn ServerHost,
            by: [f64; 3],
            cause: mod_api::MoveCause,
        ) -> Result<(), ()>;
    }

    impl EntityExt for EntityRef {
        /// A move as a teleport by the offset: no collision on the way.
        fn move_by(
            self,
            host: &mut dyn ServerHost,
            by: [f64; 3],
            _cause: mod_api::MoveCause,
        ) -> Result<(), ()> {
            let at = self.position(&*host).ok_or(())?;
            self.teleport_to(host, [at[0] + by[0], at[1] + by[1], at[2] + by[2]], None);
            Ok(())
        }
    }

    pub trait SoundExt {
        fn move_to(self, _host: &dyn ClientHost, _at: [f64; 3]) -> bool;
        fn set_pitch(self, _host: &dyn ClientHost, _pitch: f32) -> bool;
    }

    impl SoundExt for SoundRef {
        fn move_to(self, _host: &dyn ClientHost, _at: [f64; 3]) -> bool {
            false
        }
        fn set_pitch(self, _host: &dyn ClientHost, _pitch: f32) -> bool {
            false
        }
    }

    pub trait LocalPlayerExt {
        fn set_local_player_on_ground(&mut self, _on: bool) {}
        fn set_local_player_velocity(&mut self, _v: [f64; 3]) {}
        fn move_local_player_by(&mut self, _by: [f64; 3]) -> Result<Option<[f64; 3]>, ()> {
            Ok(None)
        }
    }

    impl LocalPlayerExt for dyn ClientHost + '_ {}

    pub trait PlayerDataExt {
        fn set_persistent_data_key(
            self,
            host: &mut dyn ServerHost,
            key: &str,
            value: Option<&Tag>,
        ) -> Result<(), ()>;
    }

    impl PlayerDataExt for PlayerRef {
        /// One key of the player's persistent data, through Horizon's whole-compound write.
        fn set_persistent_data_key(
            self,
            host: &mut dyn ServerHost,
            key: &str,
            value: Option<&Tag>,
        ) -> Result<(), ()> {
            let mut data = self.persistent_data(&*host).unwrap_or_default();
            match value {
                Some(v) => {
                    data.insert(key.into(), v.clone());
                }
                None => {
                    data.retain(|k, _| k.to_string_lossy() != key);
                }
            }
            self.set_persistent_data(host, data).map_err(|_| ())
        }
    }

    pub trait LevelExt {
        fn update_from_neighbour_shapes(
            self,
            _host: &mut dyn ServerHost,
            _pos: [i32; 3],
            state: BlockState,
        ) -> Result<BlockState, ()>;
        fn set_placed_by(
            self,
            _host: &mut dyn ServerHost,
            _pos: [i32; 3],
            _stack: Option<&ItemStack>,
        ) -> Result<(), ()>;
    }

    impl LevelExt for LevelRef {
        /// No shape update: the state stays as asked.
        fn update_from_neighbour_shapes(
            self,
            _host: &mut dyn ServerHost,
            _pos: [i32; 3],
            state: BlockState,
        ) -> Result<BlockState, ()> {
            Ok(state)
        }
        fn set_placed_by(
            self,
            _host: &mut dyn ServerHost,
            _pos: [i32; 3],
            _stack: Option<&ItemStack>,
        ) -> Result<(), ()> {
            Ok(())
        }
    }

    pub trait BlockEntityExt {
        fn set_shapes(self, _host: &mut dyn ServerHost, _shapes: Option<BlockEntityShapes>) -> Result<(), ()>;
    }

    impl BlockEntityExt for BlockEntityRef {
        fn set_shapes(self, _host: &mut dyn ServerHost, _shapes: Option<BlockEntityShapes>) -> Result<(), ()> {
            Ok(())
        }
    }
}
