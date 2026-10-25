//! The Lua side of the engine API: what scripts can call. Actions queue
//! a [`Command`]; reads answer from the engine's latest [`WorldView`].
//! Nothing here touches the engine directly.
//!
//! ```text
//!   actions                           → Command
//!   world.fire(name)                  → Fire { name, delay 0 }
//!   world.fire_after(seconds, name)   → Fire { name, delay }
//!   world.open(target)                → Open(target)
//!   world.close(target)               → Close(target)
//!   world.log(text), print(...)       → Log(text)
//!
//!   reads (from the snapshot; nil when there is nothing to report)
//!   world.position(target)            → {x, y, z}
//!   world.mover(target)               → {state = "open", progress = 1}
//!   world.player()                    → {x, y, z} of the feet
//!   world.time()                      → seconds since the world began
//! ```
//!
//! A `target` is an entity name, or an instance table (`self`), which
//! carries its entity's key.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Table, Value, Variadic};

use crate::view::{EntityKey, EntityView, WorldView};

/// Base-library functions that read files.
const FILE_FUNCTIONS: [&str; 2] = ["dofile", "loadfile"];
/// Global table holding the engine API.
const API_TABLE: &str = "world";
/// Instance field holding the entity key. Private by convention.
pub(crate) const ENTITY_FIELD: &str = "_entity";

/// Commands scripts have queued, shared by the API functions and the VM.
pub(crate) type Queue = Rc<RefCell<Vec<Command>>>;
/// The snapshot reads answer from, shared with the VM.
pub(crate) type SharedView = Rc<RefCell<WorldView>>;

/// Which entity an action means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Every entity with this name.
    Name(String),
    /// One entity, by key (a script passing `self`).
    Entity(EntityKey),
}

/// Something a script asked the engine to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Fire `target` after `delay` seconds (0 = next chance).
    Fire {
        /// Entity name to fire.
        target: String,
        /// Seconds to wait.
        delay: f32,
    },
    /// Start the target's mover opening, whatever its state.
    Open(Target),
    /// Start the target's mover closing, whatever its state.
    Close(Target),
    /// Log a line from the script.
    Log(String),
}

/// Installs the `world` table and a `print` that logs. Removes file
/// access.
pub(crate) fn install_api(lua: &Lua, commands: &Queue, view: &SharedView) -> mlua::Result<()> {
    let globals = lua.globals();
    for name in FILE_FUNCTIONS {
        globals.set(name, Value::Nil)?;
    }

    let api = lua.create_table()?;
    install_actions(lua, &api, commands)?;
    install_reads(lua, &api, view)?;
    globals.set(API_TABLE, api)?;

    // print(a, b) logs "a\tb", like stock Lua prints to stdout.
    let push = queue(commands);
    let tostring: Function = globals.get("tostring")?;
    let print = lua.create_function(move |_, values: Variadic<Value>| {
        let parts = values
            .into_iter()
            .map(|value| tostring.call::<String>(value))
            .collect::<mlua::Result<Vec<_>>>()?;
        push(Command::Log(parts.join("\t")));
        Ok(())
    })?;
    globals.set("print", print)
}

/// A closure that pushes onto the queue.
fn queue(commands: &Queue) -> impl Fn(Command) + 'static {
    let commands = Rc::clone(commands);
    move |command| commands.borrow_mut().push(command)
}

fn install_actions(lua: &Lua, api: &Table, commands: &Queue) -> mlua::Result<()> {
    let push = queue(commands);
    let fire = lua.create_function(move |_, target: String| {
        push(Command::Fire { target, delay: 0.0 });
        Ok(())
    })?;
    api.set("fire", fire)?;

    let push = queue(commands);
    let fire_after = lua.create_function(move |_, (delay, target): (f32, String)| {
        push(Command::Fire {
            target,
            delay: delay.max(0.0),
        });
        Ok(())
    })?;
    api.set("fire_after", fire_after)?;

    let push = queue(commands);
    let open = lua.create_function(move |_, target: Value| {
        push(Command::Open(parse_target(target)?));
        Ok(())
    })?;
    api.set("open", open)?;

    let push = queue(commands);
    let close = lua.create_function(move |_, target: Value| {
        push(Command::Close(parse_target(target)?));
        Ok(())
    })?;
    api.set("close", close)?;

    let push = queue(commands);
    let log = lua.create_function(move |_, text: String| {
        push(Command::Log(text));
        Ok(())
    })?;
    api.set("log", log)
}

fn install_reads(lua: &Lua, api: &Table, view: &SharedView) -> mlua::Result<()> {
    let shared = Rc::clone(view);
    let position = lua.create_function(move |lua, target: Value| {
        let view = shared.borrow();
        let Some(entity) = find(&view, &parse_target(target)?) else {
            return Ok(Value::Nil);
        };
        vector(lua, entity.position).map(Value::Table)
    })?;
    api.set("position", position)?;

    let shared = Rc::clone(view);
    let mover = lua.create_function(move |lua, target: Value| {
        let view = shared.borrow();
        let state = find(&view, &parse_target(target)?).and_then(|e| e.mover);
        let Some(state) = state else {
            return Ok(Value::Nil);
        };

        let table = lua.create_table()?;
        table.set("state", state.phase.as_str())?;
        table.set("progress", state.progress)?;
        Ok(Value::Table(table))
    })?;
    api.set("mover", mover)?;

    let shared = Rc::clone(view);
    let player = lua.create_function(move |lua, ()| {
        let Some(feet) = shared.borrow().player else {
            return Ok(Value::Nil);
        };
        vector(lua, feet).map(Value::Table)
    })?;
    api.set("player", player)?;

    let shared = Rc::clone(view);
    let time = lua.create_function(move |_, ()| Ok(shared.borrow().time))?;
    api.set("time", time)
}

/// A name string, or an instance table carrying its entity key.
fn parse_target(value: Value) -> mlua::Result<Target> {
    match value {
        Value::String(name) => Ok(Target::Name(name.to_str()?.to_owned())),
        Value::Table(table) => match table.raw_get::<Option<i64>>(ENTITY_FIELD)? {
            // Keys round-trip through Lua's i64 bit for bit.
            Some(key) => Ok(Target::Entity(key as EntityKey)),
            None => Err(mlua::Error::runtime("table is not an entity (no self)")),
        },
        _ => Err(mlua::Error::runtime("expected an entity name or self")),
    }
}

fn find<'a>(view: &'a WorldView, target: &Target) -> Option<&'a EntityView> {
    match target {
        Target::Name(name) => view.by_name(name),
        Target::Entity(key) => view.by_key(*key),
    }
}

fn vector(lua: &Lua, [x, y, z]: [f32; 3]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("x", x)?;
    table.set("y", y)?;
    table.set("z", z)?;
    Ok(table)
}
