//! What scripts can see of the world: a read-only snapshot the engine
//! publishes before it runs hooks. Scripts read it through `world.*`
//! functions; they never touch engine state directly.
//!
//! ```text
//!   engine: vm.set_view(WorldView { entities, player, time })
//!                     │
//!   Lua:   world.position("door1")  world.mover(self)  world.player()
//! ```
//!
//! Snapshots are cheap and replaced whole, so a hook always sees one
//! consistent moment.

/// Opaque handle the engine gives each entity, round-tripped through
/// scripts (e.g. as `self`) and back in commands.
pub type EntityKey = u64;

/// Where a mover is in its cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoverPhase {
    /// At its start.
    Closed,
    /// Moving toward its end.
    Opening,
    /// At its end.
    Open,
    /// Moving back toward its start.
    Closing,
}

impl MoverPhase {
    /// The name scripts see, e.g. "opening".
    pub fn as_str(self) -> &'static str {
        match self {
            MoverPhase::Closed => "closed",
            MoverPhase::Opening => "opening",
            MoverPhase::Open => "open",
            MoverPhase::Closing => "closing",
        }
    }
}

/// A mover's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoverView {
    /// Where in its cycle.
    pub phase: MoverPhase,
    /// 0 at its start, 1 at its end.
    pub progress: f32,
}

/// One entity as scripts see it.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityView {
    /// Its handle.
    pub key: EntityKey,
    /// Its name, if any.
    pub name: Option<String>,
    /// Where it is, meters: the middle of its solid shape, or its origin
    /// if it has none.
    pub position: [f32; 3],
    /// Its mover, if it has one.
    pub mover: Option<MoverView>,
}

/// The whole snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldView {
    /// Every entity.
    pub entities: Vec<EntityView>,
    /// The player's feet, meters; None when the player is not in the
    /// world as a body (e.g. flying).
    pub player: Option<[f32; 3]>,
    /// Seconds of simulation since the world began.
    pub time: f64,
}

impl WorldView {
    /// The entity with this handle.
    pub fn by_key(&self, key: EntityKey) -> Option<&EntityView> {
        self.entities.iter().find(|e| e.key == key)
    }

    /// The first entity with this name. A linear scan: snapshots hold a
    /// level's worth of entities, not thousands.
    pub fn by_name(&self, name: &str) -> Option<&EntityView> {
        self.entities
            .iter()
            .find(|e| e.name.as_deref() == Some(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(key: EntityKey, name: Option<&str>) -> EntityView {
        EntityView {
            key,
            name: name.map(Into::into),
            position: [0.0; 3],
            mover: None,
        }
    }

    #[test]
    fn lookups_by_key_and_first_name() {
        let view = WorldView {
            entities: vec![entity(1, Some("a")), entity(2, None), entity(3, Some("a"))],
            ..Default::default()
        };

        assert_eq!(view.by_key(2).unwrap().key, 2);
        assert_eq!(view.by_name("a").unwrap().key, 1);
        assert!(view.by_name("b").is_none() && view.by_key(9).is_none());
    }

    #[test]
    fn phases_have_script_names() {
        let names = [
            MoverPhase::Closed,
            MoverPhase::Opening,
            MoverPhase::Open,
            MoverPhase::Closing,
        ]
        .map(MoverPhase::as_str);

        assert_eq!(names, ["closed", "opening", "open", "closing"]);
    }
}
