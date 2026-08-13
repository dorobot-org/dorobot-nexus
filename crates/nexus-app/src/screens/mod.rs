//! The six surfaces.

pub mod inspect;
pub mod runs;
pub mod scene;
pub mod task;
pub mod train;
pub mod validate;

use makepad_widgets::*;

pub fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
    runs::script_mod(vm);
    scene::script_mod(vm);
    task::script_mod(vm);
    train::script_mod(vm);
    inspect::script_mod(vm);
    validate::script_mod(vm)
}
