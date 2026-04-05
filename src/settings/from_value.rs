use log::error;
use serde_json::Value;

// Trait to allow for conversion from serde_json::Value to any other data type.
// Note: Feel free to implement this trait for custom types in each subsystem.
pub trait ParseFromValue {
    fn parse_from_value(&mut self, value: Value);
}

impl ParseFromValue for f32 {
    fn parse_from_value(&mut self, value: Value) {
        if value.is_f64() {
            *self = value.as_f64().unwrap() as f32;
        } else if value.is_i64() {
            *self = value.as_i64().unwrap() as f32;
        } else if value.is_u64() {
            *self = value.as_u64().unwrap() as f32;
        } else {
            error!("Setting expected an f32, but received {value:?}");
        }
    }
}

impl ParseFromValue for u64 {
    fn parse_from_value(&mut self, value: Value) {
        if value.is_u64() {
            *self = value.as_u64().unwrap();
        } else {
            error!("Setting expected a u64, but received {value:?}");
        }
    }
}

impl ParseFromValue for u32 {
    fn parse_from_value(&mut self, value: Value) {
        if value.is_u64() {
            *self = value.as_u64().unwrap() as u32;
        } else {
            error!("Setting expected a u32, but received {value:?}");
        }
    }
}

impl ParseFromValue for i32 {
    fn parse_from_value(&mut self, value: Value) {
        if value.is_i64() {
            *self = value.as_i64().unwrap() as i32;
        } else {
            error!("Setting expected an i32, but received {value:?}");
        }
    }
}

impl ParseFromValue for String {
    fn parse_from_value(&mut self, value: Value) {
        if value.is_string() {
            *self = String::from(value.as_str().unwrap());
        } else {
            error!("Setting expected a string, but received {value:?}");
        }
    }
}

impl ParseFromValue for bool {
    fn parse_from_value(&mut self, value: Value) {
        if value.is_boolean() {
            *self = value.as_bool().unwrap();
        } else if value.is_u64() {
            *self = value.as_u64().unwrap() != 0;
        } else {
            error!("Setting expected a bool or 0/1, but received {value:?}");
        }
    }
}

impl<T: ParseFromValue + Default> ParseFromValue for Option<T> {
    fn parse_from_value(&mut self, value: Value) {
        match self.as_mut() {
            Some(inner) => inner.parse_from_value(value),
            None => {
                let mut inner = T::default();
                inner.parse_from_value(value);
                *self = Some(inner);
            }
        }
    }
}
