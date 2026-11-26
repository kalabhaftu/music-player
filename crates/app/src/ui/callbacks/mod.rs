pub mod library;
pub mod modals;
pub mod navigation;
pub mod playback;
pub mod settings;

use crate::{App, AppWindow};
use std::cell::RefCell;
use std::rc::Rc;

pub fn register_all(ui: &AppWindow, app: Rc<RefCell<App>>) {
    navigation::register(ui, app.clone());
    playback::register(ui, app.clone());
    library::register(ui, app.clone());
    modals::register(ui, app.clone());
    settings::register(ui, app.clone());
}
