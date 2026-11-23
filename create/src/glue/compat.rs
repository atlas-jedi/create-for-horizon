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
