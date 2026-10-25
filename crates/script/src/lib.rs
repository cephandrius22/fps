//! Lua scripting host. Scripts define entity classes as global tables;
//! the engine makes one instance per scripted entity and calls its
//! hooks. Scripts talk back only through commands, which the engine
//! applies after the call, so Lua never holds a reference into the
//! engine.
//!
//! ```text
//!   assets/scripts/x.lua            engine
//!   ────────────────────            ──────
//!   Counter = {}                    vm.load("scripts/x.lua", source)
//!   function Counter:on_fire()      vm.instance("Counter", Some("c1"))
//!     self.n = (self.n or 0) + 1    vm.call(&instance, Hook::Fire)
//!     world.fire("door")   ──────►  vm.drain() → [Command::Fire "door"]
//!   end
//! ```
//!
//! Lua API: `world.fire(name)`, `world.fire_after(seconds, name)`,
//! `world.log(text)`, and `print(...)` (logged). `self.name` holds the
//! entity's name. Hooks: `on_spawn`, `on_fire`, `on_use`, `on_enter`,
//! `on_tick(dt)`; a missing hook is skipped.
//!
//! Sandboxed: no `io`, `os`, `package`/`require`, `dofile` or
//! `loadfile`, and every load or hook call gets an instruction budget,
//! so a runaway loop errors instead of freezing the game. Reloading a
//! script replaces its classes; instances keep their state and pick up
//! the new methods, because they look their class up by name on every
//! access.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{
    Function, HookTriggers, Lua, LuaOptions, RegistryKey, StdLib, Table, Value, Variadic, VmState,
};

/// VM instructions one load or hook call may run before it is stopped.
/// About 10 ms of work: far more than any sane hook needs.
const INSTRUCTION_BUDGET: u32 = 1_000_000;
/// Base-library functions that read files.
const FILE_FUNCTIONS: [&str; 2] = ["dofile", "loadfile"];
/// Global table holding the engine API.
const API_TABLE: &str = "world";

/// Standard libraries scripts get. No io, os or package: scripts touch
/// the world only through `world.*`.
fn libraries() -> StdLib {
    StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH
}

/// Why a script could not load or a hook failed. Messages are Lua's,
/// with file and line.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// The source did not compile or its top level failed.
    #[error("load {chunk}: {message}")]
    Load {
        /// Script name, e.g. "scripts/sandbox.lua".
        chunk: String,
        /// Lua's report.
        message: String,
    },
    /// No global table by that name: the script defining the class did
    /// not load, or the name is misspelled.
    #[error("unknown script class {0:?}")]
    UnknownClass(String),
    /// A hook raised an error or ran past its instruction budget.
    #[error("{class}:{hook}: {message}")]
    Hook {
        /// Entity class.
        class: String,
        /// Hook name, e.g. "on_fire".
        hook: &'static str,
        /// Lua's report.
        message: String,
    },
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
    /// Log a line from the script.
    Log(String),
}

/// An entity event the engine reports to its script.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hook {
    /// The entity was just spawned.
    Spawn,
    /// Something fired the entity's name.
    Fire,
    /// The player used the entity.
    Use,
    /// The player entered the entity's trigger volume.
    Enter,
    /// One simulation step of `dt` seconds passed.
    Tick {
        /// Seconds per tick.
        dt: f32,
    },
}

impl Hook {
    /// The Lua method name, e.g. "on_fire".
    pub fn method(self) -> &'static str {
        match self {
            Hook::Spawn => "on_spawn",
            Hook::Fire => "on_fire",
            Hook::Use => "on_use",
            Hook::Enter => "on_enter",
            Hook::Tick { .. } => "on_tick",
        }
    }
}

/// One scripted entity's Lua state (its `self` table).
#[derive(Debug)]
pub struct Instance {
    table: RegistryKey,
    class: String,
}

impl Instance {
    /// The class this instance runs.
    pub fn class(&self) -> &str {
        &self.class
    }
}

/// The Lua state and the commands scripts have queued.
pub struct Vm {
    lua: Lua,
    commands: Rc<RefCell<Vec<Command>>>,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

impl Vm {
    /// Fresh sandboxed state with the `world` API installed.
    pub fn new() -> Self {
        // Safe libraries only, so this fails only if memory runs out.
        let lua = Lua::new_with(libraries(), LuaOptions::default())
            .expect("Lua state with safe standard libraries");
        let commands = Rc::new(RefCell::new(Vec::new()));

        install_api(&lua, &commands).expect("install world API into a fresh Lua state");
        Self { lua, commands }
    }

    /// Runs a script's source, defining or redefining its classes.
    /// `chunk` names it in error messages, e.g. "scripts/sandbox.lua".
    pub fn load(&mut self, chunk: &str, source: &str) -> Result<(), ScriptError> {
        let run = || self.lua.load(source).set_name(chunk).exec();

        self.budgeted(run).map_err(|err| ScriptError::Load {
            chunk: chunk.to_owned(),
            message: err.to_string(),
        })
    }

    /// New instance of `class` with `self.name = name`. Does not call
    /// `on_spawn`; the caller does, once the entity exists.
    pub fn instance(&mut self, class: &str, name: Option<&str>) -> Result<Instance, ScriptError> {
        let unknown = || ScriptError::UnknownClass(class.to_owned());

        let defined: Option<Table> = self.lua.globals().get(class).map_err(|_| unknown())?;
        if defined.is_none() {
            return Err(unknown());
        }

        let table = self.new_instance(class, name).map_err(|_| unknown())?;
        Ok(Instance {
            table,
            class: class.to_owned(),
        })
    }

    /// Calls the instance's method for `hook`, if its class has one.
    pub fn call(&mut self, instance: &Instance, hook: Hook) -> Result<(), ScriptError> {
        let failed = |err: mlua::Error| ScriptError::Hook {
            class: instance.class.clone(),
            hook: hook.method(),
            message: err.to_string(),
        };

        let this: Table = self.lua.registry_value(&instance.table).map_err(failed)?;
        let method: Value = this.get(hook.method()).map_err(failed)?;
        let Value::Function(method) = method else {
            return Ok(());
        };

        let call = || match hook {
            Hook::Tick { dt } => method.call::<()>((this, dt)),
            _ => method.call::<()>(this),
        };
        self.budgeted(call).map_err(failed)
    }

    /// True when the instance's class defines `hook`, e.g. to skip
    /// per-tick calls for classes without `on_tick`.
    pub fn has_hook(&self, instance: &Instance, hook: Hook) -> bool {
        let Ok(this) = self.lua.registry_value::<Table>(&instance.table) else {
            return false;
        };

        matches!(this.get::<Value>(hook.method()), Ok(Value::Function(_)))
    }

    /// Commands queued since the last drain, oldest first.
    pub fn drain(&mut self) -> Vec<Command> {
        std::mem::take(&mut *self.commands.borrow_mut())
    }

    /// `{ name = name }` whose missing keys resolve through the global
    /// `class` table at lookup time, so reloading a class updates every
    /// live instance.
    fn new_instance(&self, class: &str, name: Option<&str>) -> mlua::Result<RegistryKey> {
        let this = self.lua.create_table()?;
        this.set("name", name)?;

        let class_name = class.to_owned();
        let index = self
            .lua
            .create_function(move |lua, (_, key): (Table, Value)| {
                let class: Option<Table> = lua.globals().get(class_name.as_str())?;
                class.map_or(Ok(Value::Nil), |class| class.get(key))
            })?;
        let meta = self.lua.create_table()?;
        meta.set("__index", index)?;
        this.set_metatable(Some(meta))?;

        self.lua.create_registry_value(this)
    }

    /// Runs `work` with the instruction budget armed.
    fn budgeted<T>(&self, work: impl FnOnce() -> mlua::Result<T>) -> mlua::Result<T> {
        let triggers = HookTriggers::default().every_nth_instruction(INSTRUCTION_BUDGET);
        self.lua.set_hook(triggers, |_, _| {
            Err::<VmState, _>(mlua::Error::runtime(format!(
                "exceeded {INSTRUCTION_BUDGET} instructions (infinite loop?)"
            )))
        })?;

        let result = work();
        self.lua.remove_hook();
        result
    }
}

/// Installs `world.fire`, `world.fire_after`, `world.log` and a `print`
/// that logs, all queuing into `commands`. Removes file access.
fn install_api(lua: &Lua, commands: &Rc<RefCell<Vec<Command>>>) -> mlua::Result<()> {
    let globals = lua.globals();
    for name in FILE_FUNCTIONS {
        globals.set(name, Value::Nil)?;
    }

    let api = lua.create_table()?;
    let queue = |commands: &Rc<RefCell<Vec<Command>>>| {
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

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK: &str = "scripts/test.lua";

    fn vm(source: &str) -> Vm {
        let mut vm = Vm::new();
        vm.load(CHUNK, source).unwrap();
        vm
    }

    fn fire(target: &str) -> Command {
        Command::Fire {
            target: target.into(),
            delay: 0.0,
        }
    }

    const COUNTER: &str = r#"
        Counter = {}
        function Counter:on_spawn() self.n = 0 end
        function Counter:on_fire()
            self.n = self.n + 1
            world.fire(self.name .. "_" .. self.n)
        end
    "#;

    #[test]
    fn hooks_see_their_own_state_and_name() {
        let mut vm = vm(COUNTER);
        let a = vm.instance("Counter", Some("a")).unwrap();
        let b = vm.instance("Counter", Some("b")).unwrap();

        vm.call(&a, Hook::Spawn).unwrap();
        vm.call(&b, Hook::Spawn).unwrap();
        vm.call(&a, Hook::Fire).unwrap();
        vm.call(&a, Hook::Fire).unwrap();
        vm.call(&b, Hook::Fire).unwrap();

        assert_eq!(vm.drain(), [fire("a_1"), fire("a_2"), fire("b_1")]);
        assert!(vm.drain().is_empty());
    }

    #[test]
    fn missing_hooks_are_skipped() {
        let mut vm = vm(COUNTER);
        let a = vm.instance("Counter", None).unwrap();

        vm.call(&a, Hook::Use).unwrap();
        assert!(!vm.has_hook(&a, Hook::Use));
        assert!(vm.has_hook(&a, Hook::Fire));
    }

    #[test]
    fn unknown_class_is_an_error() {
        let mut vm = vm(COUNTER);
        let err = vm.instance("Nope", None).unwrap_err();

        assert!(matches!(err, ScriptError::UnknownClass(ref c) if c == "Nope"));
    }

    #[test]
    fn api_queues_every_command_kind_in_order() {
        let mut vm = vm(r#"
            Talker = {}
            function Talker:on_use()
                world.log("used")
                print("a", 1, nil)
                world.fire_after(2.5, "later")
                world.fire_after(-1, "clamped")
            end
        "#);
        let talker = vm.instance("Talker", None).unwrap();

        vm.call(&talker, Hook::Use).unwrap();

        let later = |target: &str, delay| Command::Fire {
            target: target.into(),
            delay,
        };
        assert_eq!(
            vm.drain(),
            [
                Command::Log("used".into()),
                Command::Log("a\t1\tnil".into()),
                later("later", 2.5),
                later("clamped", 0.0),
            ]
        );
    }

    #[test]
    fn tick_passes_dt() {
        let mut vm = vm(r#"
            Clock = {}
            function Clock:on_tick(dt) world.log(tostring(dt)) end
        "#);
        let clock = vm.instance("Clock", None).unwrap();

        vm.call(&clock, Hook::Tick { dt: 0.5 }).unwrap();
        assert_eq!(vm.drain(), [Command::Log("0.5".into())]);
    }

    #[test]
    fn reload_changes_behavior_but_keeps_state() {
        let mut vm = vm(COUNTER);
        let a = vm.instance("Counter", Some("a")).unwrap();
        vm.call(&a, Hook::Spawn).unwrap();
        vm.call(&a, Hook::Fire).unwrap();

        let doubled = COUNTER.replace("self.n + 1", "self.n + 10");
        vm.load(CHUNK, &doubled).unwrap();
        vm.call(&a, Hook::Fire).unwrap();

        assert_eq!(vm.drain(), [fire("a_1"), fire("a_11")]);
    }

    #[test]
    fn errors_name_the_script_or_hook() {
        let mut vm = Vm::new();
        let err = vm.load(CHUNK, "this is not lua").unwrap_err();
        assert!(err.to_string().contains(CHUNK), "{err}");

        vm.load(CHUNK, "Bad = {} function Bad:on_use() error('boom') end")
            .unwrap();
        let bad = vm.instance("Bad", None).unwrap();
        let err = vm.call(&bad, Hook::Use).unwrap_err();
        assert!(matches!(err, ScriptError::Hook { hook: "on_use", .. }));
        assert!(err.to_string().contains("boom"), "{err}");
    }

    #[test]
    fn runaway_loops_are_stopped() {
        let mut vm = vm("Spin = {} function Spin:on_tick() while true do end end");
        let spin = vm.instance("Spin", None).unwrap();

        let err = vm.call(&spin, Hook::Tick { dt: 0.1 }).unwrap_err();
        assert!(err.to_string().contains("instructions"), "{err}");

        // The budget is per call: a fresh, short call still works.
        assert!(vm.load(CHUNK, "x = 1").is_ok());
        assert!(Vm::new().load(CHUNK, "while true do end").is_err());
    }

    #[test]
    fn no_file_or_os_access() {
        let mut vm = Vm::new();
        let probe = r#"
            assert(io == nil and os == nil and require == nil)
            assert(dofile == nil and loadfile == nil)
            assert(string and table and math and utf8 and coroutine)
        "#;

        vm.load(CHUNK, probe).unwrap();
    }
}
