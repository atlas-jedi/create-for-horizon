# O que ficou desligado no port para o Horizon

Port experimental do create-sunrise para o SDK do Horizon (Nightly `f1bb66f`). Cada item: o que o jogador perde, a API do Sunrise que faltou no Horizon e onde está no código. Código desligado fica marcado com `#[cfg(any())] // Horizon: API missing` ou comentário `Horizon:`.

| Recurso no jogo | API do Sunrise que faltou | Arquivo |
|---|---|---|
| Caixa de ferramentas (toolbox): a tela própria (layout dos compartimentos, botão de depositar tudo, botão de confirmar) não existe | `MenuScreen`, `MenuScreenLayout`, `ScreenSlot`, `register_menu_screen`, `MenuButtonEvent.subject` | `src/glue/blocks.rs` |
| Extendo Grip: com o grip na mão, a mira não prefere o bloco atrás de uma entidade distante | hook `client_pick` / `ClientPickEvent` | `src/lib.rs`, `src/glue/equipment.rs` |
| Extendo Grip: o modelo animado (braços em tesoura, engrenagem girando, pose de soco) não é desenhado; o item usa o modelo base/ausente | `SpecialModelFactory`/`SpecialModelGeometry`/`SpecialModelCollector` (Horizon só aceita resposta em modelos de resource pack), `ItemModelUpdateInput`/`ItemModelLayer` | `src/glue/equipment.rs`, `src/lib.rs` |
| Filtro e Filtro de atributos: clicar com o filtro na mão não abre a tela; não dá para configurar filtros (um filtro já configurado continua valendo) | `ModMenuOpen`/`open_mod_menu` (menu com dados extras), `MenuScreen*`, `MenuPart::menu_clicked` | `src/glue/items/filters.rs`, `src/glue/items/filter_screen.rs`, `src/glue/items/mod.rs` |
| Filtro de atributos: o atributo "está na aba do criativo" nunca casa | `built_creative_tabs`/`CreativeModeTabs` | `src/glue/items/filters.rs`, `src/glue/compat.rs` |
| Schematicannon e Mesa de esquemas: as telas próprias não existem (sem botões, sem barra de progresso); shift-clique no inventário continua roteando itens | `MenuScreen*`, `register_menu_screen` | `src/glue/schematics/mod.rs`, `src/glue/schematics/screens.rs` |
| Varinha de simetria: na tela, o item aparece chapado em vez do modelo 3D girado | `Gui3dView`/`Gui3dSubject` | `src/glue/items/symmetry_screen.rs` |
| Chave inglesa, Canhão de batata, Worldshaper: a peça móvel do modelo fica parada (o Horizon pergunta o modelo uma vez por stack) | `SpecialModelRenderer` por frame (`extract_argument`/`submit`) | `src/glue/items/spinning.rs` |
| Partículas próprias (ar do ventilador/canhão de batata, indicadores, almas da assombração): a partícula não sabe a luz do lugar (almas sempre "claras") e não tem orientação própria | `ParticleBatch`/`ParticleLevelView`/`ParticleFacing`, `register_particle_type` com codecs | `src/glue/compat.rs`, `src/glue/decor/souls.rs` |
| Rolo mecânico, mochila-tanque, caixa de ferramentas, requisitor: o "ao ser colocado" só roda quando um jogador coloca o item (não quando um esquema/contraption coloca) | `BlockPart::set_placed_by` (Horizon só tem `ItemPart::on_block_placed_by`, sem nível) | `src/glue/compat.rs` e os 4 arquivos |
| Projétil do canhão de batata: sem dono (o dano não é atribuído ao jogador) | `EntitySpawn.owner`/`LevelRef::spawn_entity` (feito via `summon`) | `src/glue/compat.rs` |
| Contraptions/assentos/vagões: entidades sem reação própria a pistão (`IgnoreEntity`) | parâmetro `PushReaction` de `declare_general_entity` | `src/glue/contraptions/colliders.rs`, `seats.rs`, `trains/carts.rs`, `items/potato.rs` |
| Sons de bloco: o som não exclui mais o jogador que causou (ele pode ouvir em dobro) | último parâmetro (jogador excluído) de `LevelRef::play_sound` | vários |
| (adaptação, não corte) Dados dos block entities do Create (velocidade, itens, progresso etc.) não vão mais no NBT do block entity: ficam numa attachment de chunk (`create:entity_fields`). `/data get block` não mostra esses campos; clonar/pegar bloco com dados não leva os campos | hooks `block_entity_save_additional`/`load_additional` | `src/glue/entity_data.rs`, `src/glue/mod.rs` |
| Quebra de bloco feita pelo Create (broca, serra, etc.): sem partícula/som de quebra de bloco | `LevelRef::block_break_effect` | `src/glue/server.rs` |
| Texturas conectadas de multiblocos (tanques, cofres, casing): não se juntam entre blocos | `LevelRef::set_connection_group` | `src/glue/mod.rs` |
| `/tick freeze`: peças cinéticas não mostram o estado congelado | `tick_rate_is_frozen` | `src/glue/kinetics.rs` |
| Peças cinéticas com modelo de bloco (renderização): usam o desenho de fallback girando em vez do modelo do estado | `RenderAssets::block_state_model` | `src/glue/kinetics.rs` |
| Textos montados no servidor (ex.: link de display): saem com a chave/fallback em vez da tradução, e nomes de item pelo id | `Host::text_string`/`hover_name` só existem no cliente | `src/glue/compat.rs`, `src/glue/mod.rs` |
| Teclas: "tecla física segurada" usa o estado do mapeamento (controle de contraption pode reagir diferente) | `vanilla_key_mapping`/`mod_key_mapping` (Horizon tem `mapping`, `is_down`, `consume_click`) | `src/glue/keys.rs` |
| Fluidos do Create com lava: lava + mel não vira calcário, lava + chocolate não vira escória (vira o que o jogo faz com fluido comum) | `Registrar::fluid_interaction`/`FluidInteraction` | `src/glue/fluids/lava.rs` |
| Painel de fábrica (factory gauge): sem forma de colisão própria por painel | hook `collision_shape`, `BlockEntityRef::set_shapes` | `src/glue/stock/gauge.rs` |
| Contraptions em movimento (rolamento, pórtico, carrinhos, elevador): sem colisão com jogadores/entidades (atravessa) | colliders de entidade (`set_collider`, `collider`, `set_collider_transform`) | `src/glue/contraptions/colliders.rs`, `src/glue/compat.rs` |
| Esteira com jogador em cima / botas de mergulho: o movimento do jogador local não é empurrado no cliente (no servidor a esteira ainda move entidades) | `set_local_player_velocity`/`on_ground`/`move_local_player_by` | `src/glue/logistics/belt.rs`, `src/glue/equipment.rs` |
| Armadura de netherite de mergulho: não dá imunidade a fogo | `set_fire_immunity`/`set_client_fire_immunity` | `src/glue/equipment.rs` |
| Espada/ataque do Create: não corta o sprint do atacante | `EntityRef::set_sprinting` | `src/glue/items/sword.rs` |
| Braço mecânico e esteira: não conhecem a reação a pistão nem "pendurado" de uma entidade (pode pegar/empurrar quadros e similares) | `piston_push_reaction`, `is_hanging` | `src/glue/logistics/arms.rs`, `belt.rs` |
| Vagões (trains/carts): correção de posição vira teleporte (sem colisão no caminho) | `EntityRef::move_by` | `src/glue/trains/carts.rs` |
| Canhão de batata: o projétil não é desenhado; não sabe o dono | anexos no `EntityRenderState`, `EntityRef::owner` | `src/glue/items/potato.rs` |
| Sons contínuos (ventilador, buzina de trem): não mudam de tom nem seguem a fonte | `SoundRef::set_pitch`/`move_to` | `src/glue/processing/air.rs`, `src/glue/trains/horn.rs` |
| Chave inglesa e varinha de simetria: o bloco girado/espelhado não recalcula a forma pelos vizinhos | `LevelRef::update_from_neighbour_shapes` | `src/glue/items/wrench.rs`, `src/glue/items/symmetry.rs` |
| Schematicannon: o bloco colocado não recebe o "colocado por" (dados do item); o arquivo de upload mais antigo não é apagado; "conduz redstone" vira "é cubo cheio" | `LevelRef::set_placed_by`, `ConfigFiles::remove`, `is_redstone_conductor` | `src/glue/schematics/` |
| Cobre do Create (telhas/ladrilhos): não oxida sozinho; lixa não raspa cobre nem tira cera | `weathering_age`, `previous_weathering_stage`, `unwaxed` | `src/glue/decor/copper.rs`, `src/glue/items/sandpaper.rs` |
| Copycat: atrito do material não é aplicado; classes de bloco (escada, alçapão, grade) deduzidas pelo id | `BlockPart::friction` com nível, `block_class` | `src/glue/copycat/` |
| Mãos/garra do Create, bandeja/caixa de ferramentas: caixa de ferramentas quebrada derruba o conteúdo (em vez de guardar no item) | `PreRemoveSideEffectsEvent.keep_contents` | `src/glue/processing/containers.rs` |
| Deployer: o soco não sai com carga cheia de ataque | `PlayerActionSetup.full_attack_charge` | `src/glue/processing/deployer.rs` |
| Contraptions desenhadas: luz do nível para a estrutura inteira não é usada (luz do próprio desenho) | `BlockEntityDrawContext::level_light` | `src/glue/contraptions/draw.rs` |
| Conquistas/cinto de ferramentas: dados persistentes do jogador gravados via compound inteiro (adaptação) | `PlayerRef::set_persistent_data_key` | `src/glue/compat.rs` |
