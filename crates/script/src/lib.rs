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
//!   function Counter:on_fire()      vm.instance("Counter", Some("c1"), key)
//!     self.n = (self.n or 0) + 1    vm.call(&instance, Hook::Fire)
//!     world.fire("door")   ──────►  vm.drain() → [Command::Fire "door"]
//!   end
//! ```
//!
//! Lua API (see `api.rs`): actions `world.fire`, `world.fire_after`,
//! `world.open`, `world.close`, `world.log` and `print`; reads
//! `world.position`, `world.mover`, `world.player` and `world.time`,
//! answered from the snapshot the engine publishes with
//! [`Vm::set_view`]. `self.name` holds the entity's name, and `self`
//! itself works as a target. Hooks: `on_spawn`, `on_fire`, `on_use`,
//! `on_enter`, `on_tick(dt)`; a missing hook is skipped.
//!
//! Sandboxed: no `io`, `os`, `package`/`require`, `dofile` or
//! `loadfile`, and every load or hook call gets an instruction budget,
//! so a runaway loop errors instead of freezing the game. Reloading a
//! script replaces its classes; instances keep their state and pick up
//! the new methods, because they look their class up by name on every
//! access.

mod api;
mod view;

use mlua::{HookTriggers, Lua, LuaOptions, RegistryKey, StdLib, Table, Value, VmState};

pub use api::{Command, Target};
pub use view::{EntityKey, EntityView, MoverPhase, MoverView, WorldView};

use api::{ENTITY_FIELD, Queue, SharedView, install_api};

/// VM instructions one load or hook call may run before it is stopped.
/// About 10 ms of work: far more than any sane hook needs.
const INSTRUCTION_BUDGET: u32 = 1_000_000;

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
    commands: Queue,
    view: SharedView,
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
        let commands = Queue::default();
        let view = SharedView::default();

        install_api(&lua, &commands, &view).expect("install world API into a fresh Lua state");
        Self {
            lua,
            commands,
            view,
        }
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

    /// New instance of `class` for entity `key`, with `self.name = name`.
    /// Does not call `on_spawn`; the caller does, once the entity exists.
    pub fn instance(
        &mut self,
        class: &str,
        name: Option<&str>,
        key: EntityKey,
    ) -> Result<Instance, ScriptError> {
        let unknown = || ScriptError::UnknownClass(class.to_owned());

        let defined: Option<Table> = self.lua.globals().get(class).map_err(|_| unknown())?;
        if defined.is_none() {
            return Err(unknown());
        }

        let table = self.new_instance(class, name, key).map_err(|_| unknown())?;
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

    /// Replaces the snapshot `world.*` reads answer from. Call before
    /// running hooks, so they see the current moment.
    pub fn set_view(&mut self, view: WorldView) {
        *self.view.borrow_mut() = view;
    }

    /// Commands queued since the last drain, oldest first.
    pub fn drain(&mut self) -> Vec<Command> {
        std::mem::take(&mut *self.commands.borrow_mut())
    }

    /// `{ name = name }` whose missing keys resolve through the global
    /// `class` table at lookup time, so reloading a class updates every
    /// live instance.
    fn new_instance(
        &self,
        class: &str,
        name: Option<&str>,
        key: EntityKey,
    ) -> mlua::Result<RegistryKey> {
        let this = self.lua.create_table()?;
        this.set("name", name)?;
        // The key as Lua's i64, bit for bit; api.rs turns it back.
        this.set(ENTITY_FIELD, key as i64)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK: &str = "scripts/test.lua";
    /// Entity key for instances whose entity does not matter.
    const KEY: EntityKey = 7;

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
        let a = vm.instance("Counter", Some("a"), KEY).unwrap();
        let b = vm.instance("Counter", Some("b"), KEY).unwrap();

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
        let a = vm.instance("Counter", None, KEY).unwrap();

        vm.call(&a, Hook::Use).unwrap();
        assert!(!vm.has_hook(&a, Hook::Use));
        assert!(vm.has_hook(&a, Hook::Fire));
    }

    #[test]
    fn unknown_class_is_an_error() {
        let mut vm = vm(COUNTER);
        let err = vm.instance("Nope", None, KEY).unwrap_err();

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
        let talker = vm.instance("Talker", None, KEY).unwrap();

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
        let clock = vm.instance("Clock", None, KEY).unwrap();

        vm.call(&clock, Hook::Tick { dt: 0.5 }).unwrap();
        assert_eq!(vm.drain(), [Command::Log("0.5".into())]);
    }

    #[test]
    fn reload_changes_behavior_but_keeps_state() {
        let mut vm = vm(COUNTER);
        let a = vm.instance("Counter", Some("a"), KEY).unwrap();
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
        let bad = vm.instance("Bad", None, KEY).unwrap();
        let err = vm.call(&bad, Hook::Use).unwrap_err();
        assert!(matches!(err, ScriptError::Hook { hook: "on_use", .. }));
        assert!(err.to_string().contains("boom"), "{err}");
    }

    #[test]
    fn runaway_loops_are_stopped() {
        let mut vm = vm("Spin = {} function Spin:on_tick() while true do end end");
        let spin = vm.instance("Spin", None, KEY).unwrap();

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

    fn door_view() -> WorldView {
        WorldView {
            entities: vec![EntityView {
                key: KEY,
                name: Some("door".into()),
                position: [1.0, 2.0, 3.0],
                mover: Some(MoverView {
                    phase: MoverPhase::Opening,
                    progress: 0.5,
                }),
            }],
            player: Some([4.0, 0.0, -6.0]),
            time: 12.5,
        }
    }

    /// Runs `body` as `Probe:on_use()` on an instance of entity KEY and
    /// returns what it logged.
    fn probe(view: WorldView, body: &str) -> Vec<Command> {
        let mut vm = vm(&format!("Probe = {{}} function Probe:on_use() {body} end"));
        vm.set_view(view);
        let probe = vm.instance("Probe", None, KEY).unwrap();

        vm.call(&probe, Hook::Use).unwrap();
        vm.drain()
    }

    fn logged(text: &str) -> Vec<Command> {
        vec![Command::Log(text.into())]
    }

    #[test]
    fn reads_positions_by_name_or_self() {
        let by_name = "local p = world.position('door') print(p.x, p.y, p.z)";
        assert_eq!(probe(door_view(), by_name), logged("1.0\t2.0\t3.0"));

        let by_self = "print(world.position(self).z)";
        assert_eq!(probe(door_view(), by_self), logged("3.0"));
    }

    #[test]
    fn reads_mover_state_player_and_time() {
        let body = "local m = world.mover('door') print(m.state, m.progress, world.player().z, world.time())";
        assert_eq!(probe(door_view(), body), logged("opening\t0.5\t-6.0\t12.5"));
    }

    #[test]
    fn missing_things_read_as_nil() {
        let body = "print(world.position('nobody'), world.mover('nobody'), world.player())";
        let empty = WorldView::default();

        assert_eq!(probe(empty, body), logged("nil\tnil\tnil"));
    }

    #[test]
    fn open_and_close_queue_commands_for_names_and_self() {
        let commands = probe(door_view(), "world.open('door') world.close(self)");

        assert_eq!(
            commands,
            [
                Command::Open(Target::Name("door".into())),
                Command::Close(Target::Entity(KEY)),
            ]
        );
    }

    #[test]
    fn a_bad_target_is_a_script_error() {
        let mut vm = vm("Bad = {} function Bad:on_use() world.open(42) end");
        let bad = vm.instance("Bad", None, KEY).unwrap();

        let err = vm.call(&bad, Hook::Use).unwrap_err();
        assert!(err.to_string().contains("entity name or self"), "{err}");
    }

    #[test]
    fn keys_survive_the_trip_through_lua() {
        let key = EntityKey::MAX - 1;
        let mut vm = vm("K = {} function K:on_use() world.close(self) end");
        let k = vm.instance("K", None, key).unwrap();

        vm.call(&k, Hook::Use).unwrap();
        assert_eq!(vm.drain(), [Command::Close(Target::Entity(key))]);
    }
}
