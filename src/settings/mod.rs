pub mod font;
mod from_value;
mod window_size;

use parking_lot::RwLock;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt::Debug,
};

pub use from_value::ParseFromValue;
pub use window_size::{
    DEFAULT_GRID_SIZE, MIN_GRID_SIZE, PersistentWindowSettings, clamped_grid_size,
    load_last_window_settings, save_window_size, termvide_std_datapath,
};

pub mod config;
pub use config::{
    AppHotReloadConfigs, Config, HotReloadConfigs, RendererHotReloadConfigs, WindowHotReloadConfigs,
};

pub trait SettingGroup {
    type ChangedEvent: Debug + Clone + Send + Sync + Any;
    fn register(settings: &Settings);
}

#[derive(Clone, Debug)]
pub struct FontConfigState {
    pub has_font: bool,
}

impl FontConfigState {
    pub fn new() -> Self {
        Self { has_font: false }
    }
}

#[derive(Default, Debug)]
pub struct Settings {
    settings: RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
}

impl Settings {
    pub fn new() -> Self {
        let settings = Self::default();
        settings.set(&FontConfigState::new());
        settings
    }

    pub fn set<T: Clone + Send + Sync + 'static>(&self, t: &T) {
        let type_id: TypeId = TypeId::of::<T>();
        let t: T = (*t).clone();
        let mut write_lock = self.settings.write();
        write_lock.insert(type_id, Box::new(t));
    }

    pub fn get<T: Clone + Send + Sync + 'static>(&self) -> T {
        let read_lock = self.settings.read();
        let boxed = &read_lock
            .get(&TypeId::of::<T>())
            .expect("Trying to retrieve a settings object that doesn't exist: {:?}");
        let value: &T = boxed
            .downcast_ref::<T>()
            .expect("Attempted to extract a settings object of the wrong type");
        (*value).clone()
    }

    pub fn register<T: SettingGroup>(&self) {
        T::register(self);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsChanged {
    Window(crate::window::WindowSettingsChanged),
    Cursor(crate::renderer::cursor_renderer::CursorSettingsChanged),
    Renderer(crate::renderer::RendererSettingsChanged),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set() {
        let settings = Settings::new();

        let v1: u32 = 1;
        let v2: f32 = 1.0;
        let vt1 = TypeId::of::<u32>();
        let vt2 = TypeId::of::<f32>();
        let v3: u32 = 2;

        {
            settings.set(&v1);

            let values = settings.settings.read();
            let r1 = values.get(&vt1).unwrap().downcast_ref::<u32>().unwrap();
            assert_eq!(v1, *r1);
        }

        {
            settings.set(&v2);
            settings.set(&v3);

            let values = settings.settings.read();
            let r2 = values.get(&vt1).unwrap().downcast_ref::<u32>().unwrap();
            let r3 = values.get(&vt2).unwrap().downcast_ref::<f32>().unwrap();

            assert_eq!(v3, *r2);
            assert_eq!(v2, *r3);
        }
    }

    #[test]
    fn test_get() {
        let settings = Settings::new();

        let v1: u32 = 1;
        let v2: f32 = 1.0;
        let vt1 = TypeId::of::<u32>();
        let vt2 = TypeId::of::<f32>();

        let mut values = settings.settings.write();
        values.insert(vt1, Box::new(v1));
        values.insert(vt2, Box::new(v2));

        unsafe {
            settings.settings.force_unlock_write();
        }

        let r1 = settings.get::<u32>();
        let r2 = settings.get::<f32>();

        assert_eq!(v1, r1);
        assert_eq!(v2, r2);
    }
}
