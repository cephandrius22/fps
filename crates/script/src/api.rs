//! The Lua side of the engine API: what scripts can call. Every call
//! queues a [`Command`]; nothing here touches the engine directly.
//!
//! ```text
//!   world.fire(name)                 → Command::Fire { name, delay 0 }
//!   world.fire_after(seconds, name)  → Command::Fire { name, delay }
//!   world.log(text), print(...)      → Command::Log(text)
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Value, Variadic};

/// Base-library functions that read files.
const FILE_FUNCTIONS: [&str; 2] = ["dofile", "loadfile"];
/// Global table holding the engine API.
const API_TABLE: &str = "world";

/// Commands scripts have queued, shared by the API functions and the VM.
pub(crate) type Queue = Rc<RefCell<Vec<Command>>>;

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
    /// Log a line from the script.
    Log(String),
}

/// Installs `world.fire`, `world.fire_after`, `world.log` and a `print`
/// that logs, all queuing into `commands`. Removes file access.
pub(crate) fn install_api(lua: &Lua, commands: &Queue) -> mlua::Result<()> {
    let globals = lua.globals();
    for name in FILE_FUNCTIONS {
        globals.set(name, Value::Nil)?;
    }

    let api = lua.create_table()?;
    let queue = |commands: &Queue| {
        let commands = Rc::clone(commands);
        move |command| commands.borrow_mut().push(command)
    };

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
    let log = lua.create_function(move |_, text: String| {
        push(Command::Log(text));
        Ok(())
    })?;
    api.set("log", log)?;
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
