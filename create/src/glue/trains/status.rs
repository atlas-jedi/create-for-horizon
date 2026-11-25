//! What a train tells its owner, the player who assembled it (`TrainStatus.tick`): the
//! engine's queue (`create_engine::train_status`), told in their chat under the train's name
//! once they are on, wherever they are; kept (the last three) while they are away.

use super::super::server::ServerLevel;
use super::super::world::with_world;
use create_engine::train_status::{Arg, HEADER, Line, Message};
use mod_api::Text;
use mod_api::text::TextStyle;

/// `Train.owner`: `StationBlockEntity.assemble(playerUUID)`'s player owns the train standing at
/// station `pos`, assembled just now.
pub fn set_owner(dim: usize, pos: [i32; 3], owner: u128) {
    let Some(train) = super::assembly::present_train(dim, pos) else {
        return;
    };
    with_world(|w| {
        if let Some(t) = w.dims[dim]
            .railway
            .trains
            .iter_mut()
            .find(|t| t.id == train)
        {
            t.owner = Some(owner);
        }
    });
}

fn colored(text: Text, color: &str) -> Text {
    let mut style = TextStyle::default();
    style.color = Some(color.to_owned());
    text.with_style(style)
}

/// ` - ` in gray and the line in its colour (`displayInformation`, `StatusMessage`).
fn line_text(l: &Line, level: &str) -> Text {
    let args = l
        .args
        .iter()
        .map(|a| match a {
            Arg::Text(t) => Text::literal(t.as_str()),
            Arg::Level => Text::literal(level),
        })
        .collect();
    let body = colored(
        Text::translate(&l.translation(), args),
        &format!("#{:06X}", l.color()),
    );
    colored(Text::literal(" - "), "gray").append(body)
}

/// `TrainStatus.tick` after the railway's tick: each train with something to tell whose owner
/// is on tells them, its name in gold first (`train.status`), then each message's lines.
pub fn tell(lv: &mut ServerLevel<'_>) {
    let dim = lv.dim;
    let host = &*lv.host;
    let due: Vec<(u128, String, Vec<Message>)> = with_world(|w| {
        w.dims[dim]
            .railway
            .trains
            .iter_mut()
            .filter(|t| !t.status.queued.is_empty())
            .filter_map(|t| {
                let owner = t.owner?;
                host.player(owner)?;
                Some((owner, t.name.clone(), t.status.take()))
            })
            .collect()
    })
    .unwrap_or_default();
    for (owner, name, messages) in due {
        let Some(player) = lv.host.player(owner) else {
            continue;
        };
        // `Train.name`: an unnamed train is `train.unnamed`.
        let name = if name.is_empty() {
            Text::translate("create.train.unnamed", Vec::new())
        } else {
            Text::literal(name.as_str())
        };
        let header = colored(Text::translate(HEADER, vec![name]), "gold");
        let _ = player.tell(lv.host, header);
        for line in messages.iter().flatten() {
            let _ = player.tell(lv.host, line_text(line, lv.dimension));
        }
    }
}
