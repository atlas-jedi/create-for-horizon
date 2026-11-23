# O que ficou desligado no port para o Horizon

Port experimental do create-sunrise para o SDK do Horizon (Nightly `f1bb66f`). Cada item: o que o jogador perde, a API do Sunrise que faltou no Horizon e onde está no código. Código desligado fica marcado com `#[cfg(any())] // Horizon: API missing` ou comentário `Horizon:`.

| Recurso no jogo | API do Sunrise que faltou | Arquivo |
|---|---|---|
| Caixa de ferramentas (toolbox): a tela própria (layout dos compartimentos, botão de depositar tudo, botão de confirmar) não existe | `MenuScreen`, `MenuScreenLayout`, `ScreenSlot`, `register_menu_screen`, `MenuButtonEvent.subject` | `src/glue/blocks.rs` |
| Extendo Grip: com o grip na mão, a mira não prefere o bloco atrás de uma entidade distante | hook `client_pick` / `ClientPickEvent` | `src/lib.rs`, `src/glue/equipment.rs` |
| Extendo Grip: o modelo animado (braços em tesoura, engrenagem girando, pose de soco) não é desenhado; o item usa o modelo base/ausente | `SpecialModelFactory`/`SpecialModelGeometry`/`SpecialModelCollector` (Horizon só aceita resposta em modelos de resource pack), `ItemModelUpdateInput`/`ItemModelLayer` | `src/glue/equipment.rs`, `src/lib.rs` |
