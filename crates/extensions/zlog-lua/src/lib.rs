mod vm;

use std::sync::Arc;

use zlog_cfg::script::{Limits, ScriptEngine, Vm};

pub use vm::LuaVm;

pub const LANGUAGE: &str = "LUA";

pub struct Lua;

impl ScriptEngine for Lua {
    fn language(&self) -> &str {
        LANGUAGE
    }

    fn versions(&self) -> &[u32] {
        &[1]
    }

    fn compile(
        &self,
        name: &str,
        source: &str,
        version: u32,
        limits: &Limits,
    ) -> Result<Box<dyn Vm>, String> {
        if !self.versions().contains(&version) {
            return Err(format!("SCRIPT LUA {version} is not supported"));
        }
        Ok(Box::new(LuaVm::compile(name, source, limits)?))
    }
}

pub fn install() {
    zlog_cfg::script::add_engine(Arc::new(Lua));
}

pub fn register(b: &mut zlog_ui::Builder) {
    zlog_cfg::script::register_engine(b, Arc::new(Lua));
}
