//! Create's own block entity fields (`SmartBlockEntity.write` and `read`) in each entity's saved
//! data, through `saveAdditional` and `loadAdditional`, so a chunk save, `/data`, loot and
//! pick-block-with-data all see them.
//!
//! A subsystem writes the fields of its block entities in a column
//! ([`Subsystem::save_entities`]) when the column is published; what it wrote is kept here by
//! entity and given to the entity's `saveAdditional`. The save hook cannot reach the item
//! registry a stack's NBT needs, so the fields are made at publication, which is also when a
//! changed entity is marked for its chunk's next save (`setChanged`). Saved data an entity loads
//! is given to every subsystem ([`Subsystem::load_entity`]) once its column's legacy data is in,
//! as Create's `read` runs before the block entity first ticks.

use super::SUBSYSTEMS;
use super::server::ServerLevel;
use super::states;
use super::subsystem::Subsystem;
use super::world::with_world;
use create_engine::Block;
use mod_api::{
    BlockEntityLoadAdditionalEvent, BlockEntityPart, BlockEntitySaveAdditionalEvent,
    BlockEntityTypeKey, BlockPart, Compound, LevelHost, LevelRef, Registrar, Tag,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

/// What the subsystems write for the block entities of one column, by position.
pub type Entities = BTreeMap<[i32; 3], Compound>;

/// Marks data saved with the fields: what an older version put under the same keys (a drawn
/// filter's id, say) is not read as them, so its column's data stands. Data without it is read
/// unless it came with a column an older version saved (placing with data, `/data` and cloning
/// give a block entity Create's own keys alone, as `loadWithComponents` reads them).
const MARKER: &str = "create:fields";

/// A block entity's fields as last published or loaded, and the block they were written for.
#[derive(Clone, Debug, PartialEq)]
struct Record {
    block: Block,
    fields: Compound,
}

/// Data a block entity loaded: its level and position, its fields, and whether they are known
/// to be them ([`loaded`]).
type Loaded = (LevelRef, [i32; 3], Record, bool);

thread_local! {
    /// Each level's index and game time at its last publication, for the save hook's live fields.
    static CLOCKS: RefCell<HashMap<LevelRef, (usize, i64)>> = RefCell::new(HashMap::new());
    static RECORDS: RefCell<HashMap<(LevelRef, [i32; 3]), Record>> = RefCell::new(HashMap::new());
    /// Data block entities loaded that the subsystems have not read yet, and whether it is
    /// known to be the fields (saved with the marker, or loaded into an entity that has some).
    static PENDING: RefCell<Vec<Loaded>> = const { RefCell::new(Vec::new()) };
}

/// Adds the saved-fields part to block entity type `kind`: every type of Create's has it.
pub fn attach(r: &mut dyn Registrar, kind: &BlockEntityTypeKey) {
    if let Err(e) = r.block_entity_part(kind, SavedFields) {
        crate::create_log!("[create] the saved-fields part was refused: {e:?}");
    }
}

fn keys<'a>(
    systems: &'a [&'a dyn Subsystem],
    block: Block,
) -> impl Iterator<Item = &'static str> + 'a {
    systems
        .iter()
        .flat_map(move |s| s.saved_keys(block).iter().copied())
        .chain([MARKER])
}

fn own_fields(systems: &[&dyn Subsystem], block: Block, tag: &Compound) -> Compound {
    keys(systems, block)
        .filter_map(|k| Some((k.into(), tag.get(&k.into())?.clone())))
        .collect()
}

struct SavedFields;

#[mod_api::horizon_part]
impl BlockPart for SavedFields {}

#[mod_api::horizon_part]
impl BlockEntityPart for SavedFields {
    fn block_entity_save_additional(
        &self,
        level: &dyn LevelHost,
        event: &mut BlockEntitySaveAdditionalEvent<'_>,
    ) {
        let block = level
            .block_state(event.pos)
            .and_then(|s| states::level_block(level, s));
        write_record(SUBSYSTEMS, event.level, event.pos, block, event.tag);
    }

    fn block_entity_load_additional(&self, event: &mut BlockEntityLoadAdditionalEvent<'_>) {
        let Some((at, pos, state)) = event
            .subject
            .with_view(|v| (v.level(), v.position(), v.state()))
        else {
            return;
        };
        let level = event.subject.host();
        let Some(block) = states::level_block(level, state) else {
            return;
        };
        let known = RECORDS.with_borrow(|r| r.contains_key(&(at, pos)));
        let Some((fields, sure)) = loaded(SUBSYSTEMS, block, event.tag, known) else {
            return;
        };
        PENDING.with_borrow_mut(|p| {
            p.retain(|(l, q, _, _)| (*l, *q) != (at, pos));
            p.push((at, pos, Record { block, fields }, sure));
        });
    }
}

/// What `loadAdditional` gives `block`'s subsystems to read from `tag`: its own fields, and
/// whether they are known to be them (saved with the marker, or loaded into an entity that had
/// fields, `known`). None when the tag holds none of them.
fn loaded(
    systems: &[&dyn Subsystem],
    block: Block,
    tag: &Compound,
    known: bool,
) -> Option<(Compound, bool)> {
    let fields = own_fields(systems, block, tag);
    let marker = MARKER.into();
    if !known && fields.keys().all(|k| *k == marker) {
        return None;
    }
    Some((fields, known || tag.contains_key(&marker)))
}

/// Whether loaded fields are read: unless they came without the marker in a column an older
/// version saved (`legacy`), whose block entities held what it drew under the same keys.
fn read(sure: bool, legacy: bool) -> bool {
    sure || !legacy
}

/// `saveAdditional`: the entity's record replaces its block's saved keys (none for an entity
/// without one), unless data it loaded is still to be read (the tag is that data then). Every
/// entity saves the marker, so `/data merge` on one that never had fields is read.
fn write_record(
    systems: &[&dyn Subsystem],
    level: LevelRef,
    pos: [i32; 3],
    block: Option<Block>,
    tag: &mut Compound,
) {
    if PENDING.with_borrow(|p| p.iter().any(|(l, q, _, _)| (*l, *q) == (level, pos))) {
        return;
    }
    let record = RECORDS.with_borrow(|r| r.get(&(level, pos)).cloned());
    let block = record.as_ref().map(|r| r.block).or(block);
    if let Some(block) = block {
        for k in keys(systems, block) {
            tag.remove(&k.into());
        }
    }
    if let Some(record) = record {
        tag.extend(record.fields);
    }
    if let Some(block) = block
        && let Some((dim, now)) = CLOCKS.with_borrow(|c| c.get(&level).copied())
    {
        for s in systems {
            s.live_fields(dim, now, pos, block, tag);
        }
    }
    tag.insert(MARKER.into(), Tag::Byte(1));
}

/// Sets `inner` to `value` in the compound field `outer` of `fields`, when the record holds one.
pub fn patch(fields: &mut Compound, outer: &str, inner: &str, value: Tag) {
    if let Some(Tag::Compound(c)) = fields.get_mut(&outer.into()) {
        c.insert(inner.into(), value);
    }
}

/// The game time `lv` publishes at: what the save hook's live fields are counted from.
pub fn note_clock(lv: &ServerLevel<'_>) {
    let now = lv.game_time();
    CLOCKS.with_borrow_mut(|c| {
        c.insert(lv.level, (lv.dim, now));
    });
}

/// The columns of `lv` holding loaded data still to be read.
pub fn pending_columns(lv: &ServerLevel<'_>) -> Vec<[i32; 2]> {
    let mut columns: Vec<[i32; 2]> = PENDING.with_borrow(|p| {
        p.iter()
            .filter(|(l, _, _, _)| *l == lv.level)
            .map(|(_, q, _, _)| [q[0] >> 4, q[2] >> 4])
            .collect()
    });
    columns.sort_unstable();
    columns.dedup();
    columns
}

/// `SmartBlockEntity.read` for each block entity of column `chunk` of `lv` that loaded data;
/// `legacy`: the column's data is an older version's, so data without the marker is not read.
pub fn load(lv: &mut ServerLevel<'_>, chunk: [i32; 2], legacy: bool) {
    let level = lv.level;
    let loaded: Vec<([i32; 3], Record)> = PENDING.with_borrow_mut(|p| {
        let (mine, rest) = std::mem::take(p)
            .into_iter()
            .partition(|(l, q, _, _)| *l == level && [q[0] >> 4, q[2] >> 4] == chunk);
        *p = rest;
        mine.into_iter()
            .filter(|(_, _, _, sure)| read(*sure, legacy))
            .map(|(_, q, r, _)| (q, r))
            .collect()
    });
    for (pos, record) in loaded {
        for s in SUBSYSTEMS {
            s.load_entity(lv, pos, record.block, &record.fields);
        }
        RECORDS.with_borrow_mut(|r| r.insert((level, pos), record));
    }
}

/// `SmartBlockEntity.write` for column `chunk` of `lv`: each block entity whose fields changed
/// keeps the new ones and is marked for its chunk's next save.
pub fn save(lv: &mut ServerLevel<'_>, chunk: [i32; 2]) {
    let mut fresh = Entities::new();
    for s in SUBSYSTEMS {
        s.save_entities(lv, chunk, &mut fresh);
    }
    let fresh: Vec<([i32; 3], Record)> = fresh
        .into_iter()
        .filter_map(|(pos, mut fields)| {
            fields.insert(MARKER.into(), Tag::Byte(1));
            let id = lv.block(pos);
            let block = with_world(|w| w.table.get(id).map(|(b, _)| b)).flatten()?;
            Some((pos, Record { block, fields }))
        })
        .collect();
    for pos in changed(lv.level, chunk, fresh) {
        let marked = lv
            .host
            .block_entity(lv.level, pos)
            .is_some_and(|e| e.set_changed(&mut *lv.host).is_ok());
        if !marked {
            RECORDS.with_borrow_mut(|r| r.remove(&(lv.level, pos)));
        }
    }
}

fn changed(level: LevelRef, chunk: [i32; 2], fresh: Vec<([i32; 3], Record)>) -> Vec<[i32; 3]> {
    let inside = |p: &[i32; 3]| [p[0] >> 4, p[2] >> 4] == chunk;
    let mut fresh: HashMap<[i32; 3], Record> = fresh.into_iter().collect();
    RECORDS.with_borrow_mut(|r| {
        let mut changed: Vec<[i32; 3]> = r
            .iter_mut()
            .filter(|((l, p), _)| *l == level && inside(p))
            .filter_map(|((_, p), old)| {
                let new = fresh.remove(p).unwrap_or(Record {
                    block: old.block,
                    fields: Compound::new(),
                });
                (*old != new).then(|| {
                    *old = new;
                    *p
                })
            })
            .collect();
        for (p, new) in fresh {
            if !new.fields.is_empty() {
                r.insert((level, p), new);
                changed.push(p);
            }
        }
        changed.sort_unstable();
        changed
    })
}

/// Splits column data whose lists hold compounds of block entities, each with its position under
/// `pos_key`, into the entities' fields: for each `(list, key)` of `names`, the entries of `list`
/// at a position, without it, are the list under `key` of the entity there.
pub fn split(lists: &Compound, pos_key: &str, names: &[(&str, &str)], out: &mut Entities) {
    for (list, key) in names {
        let Some(Tag::List(entries)) = lists.get(&(*list).into()) else {
            continue;
        };
        for entry in entries {
            let Tag::Compound(c) = entry else {
                continue;
            };
            let mut c = c.clone();
            let Some(Tag::IntArray(p)) = c.remove(&pos_key.into()) else {
                continue;
            };
            let Ok(pos) = <[i32; 3]>::try_from(p.as_slice()) else {
                continue;
            };
            let fields = out.entry(pos).or_default();
            if let Tag::List(l) = fields
                .entry((*key).into())
                .or_insert_with(|| Tag::List(Vec::new()))
            {
                l.push(Tag::Compound(c));
            }
        }
    }
}

/// What [`split`] took from column data for the block entity at `pos`, as column data again
/// (its entries at `pos`), for the column's reader.
pub fn join(tag: &Compound, pos: [i32; 3], pos_key: &str, names: &[(&str, &str)]) -> Compound {
    names
        .iter()
        .filter_map(|(list, key)| {
            let Tag::List(entries) = tag.get(&(*key).into())? else {
                return None;
            };
            let entries = entries
                .iter()
                .filter_map(|e| {
                    let Tag::Compound(c) = e else { return None };
                    let mut c = c.clone();
                    c.insert(pos_key.into(), Tag::IntArray(pos.to_vec()));
                    Some(Tag::Compound(c))
                })
                .collect();
            Some(((*list).into(), Tag::List(entries)))
        })
        .collect()
}

/// Column `chunk` of `level` left: its block entities were saved with it.
pub fn column_unloaded(level: LevelRef, chunk: [i32; 2]) {
    let outside = |p: &[i32; 3]| [p[0] >> 4, p[2] >> 4] != chunk;
    RECORDS.with_borrow_mut(|r| r.retain(|(l, p), _| *l != level || outside(p)));
    PENDING.with_borrow_mut(|p| p.retain(|(l, q, _, _)| *l != level || outside(q)));
}

/// The world closed.
pub fn clear() {
    CLOCKS.with_borrow_mut(HashMap::clear);
    RECORDS.with_borrow_mut(HashMap::clear);
    PENDING.with_borrow_mut(Vec::clear);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_fields_never_share_a_key_with_what_clients_are_sent() {
        let synced: std::collections::HashSet<&str> = SUBSYSTEMS
            .iter()
            .flat_map(|s| s.synced_keys().iter().copied())
            .collect();
        for block in Block::ALL {
            let valve = create_engine::dye::is_valve_handle(block);
            for key in keys(SUBSYSTEMS, block) {
                let handle_only = key == super::super::sources::handles::SCROLL_VALUE;
                if handle_only && !valve {
                    continue;
                }
                assert!(!synced.contains(key), "{block:?} saves synced key {key}");
            }
        }
    }

    #[test]
    fn data_placed_without_the_marker_is_read_outside_an_older_column() {
        let filter = Tag::Compound(Compound::from([(
            "id".into(),
            Tag::String("minecraft:diamond".into()),
        )]));
        // `block_entity_data` on an item: Create's keys alone, as `loadWithComponents` reads them.
        let placed = Compound::from([
            ("id".into(), Tag::String("create:kinetic".into())),
            ("Filter".into(), filter.clone()),
        ]);
        let (fields, sure) = loaded(SUBSYSTEMS, Block::BrassFunnel, &placed, false).unwrap();
        assert_eq!(fields.get(&"Filter".into()), Some(&filter));
        assert!(fields.get(&"id".into()).is_none());
        assert!(!sure);
        assert!(read(sure, false));
        assert!(!read(sure, true));
        // Saved with the fields, or loaded into an entity that had them: always read.
        let mut saved = placed.clone();
        saved.insert(MARKER.into(), Tag::Byte(1));
        let (_, sure) = loaded(SUBSYSTEMS, Block::BrassFunnel, &saved, false).unwrap();
        assert!(sure && read(sure, true));
        let (_, sure) = loaded(SUBSYSTEMS, Block::BrassFunnel, &placed, true).unwrap();
        assert!(sure && read(sure, true));
        // Nothing of the block's own: nothing to read.
        let other = Compound::from([("id".into(), Tag::String("create:kinetic".into()))]);
        assert!(loaded(SUBSYSTEMS, Block::BrassFunnel, &other, false).is_none());
        let marked = Compound::from([(MARKER.into(), Tag::Byte(1))]);
        assert!(loaded(SUBSYSTEMS, Block::BrassFunnel, &marked, false).is_none());
    }

    /// `MillstoneBlockEntity.write` saves its `Timer`, so a millstone's milling goes on after a
    /// reload: the field is its block entity's own, read back with the rest.
    #[test]
    fn a_millstone_reads_back_its_timer() {
        let saved = Compound::from([
            ("id".into(), Tag::String("create:kinetic".into())),
            ("Timer".into(), Tag::Int(87)),
            (MARKER.into(), Tag::Byte(1)),
        ]);
        let (fields, sure) = loaded(SUBSYSTEMS, Block::Millstone, &saved, false).unwrap();
        assert!(sure);
        assert_eq!(fields.get(&"Timer".into()), Some(&Tag::Int(87)));
        // Not another block's.
        assert!(
            loaded(SUBSYSTEMS, Block::Basin, &saved, false)
                .is_none_or(|(f, _)| !f.contains_key(&"Timer".into()))
        );
    }
}
