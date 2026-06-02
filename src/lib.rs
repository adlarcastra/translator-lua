use std::collections::HashMap;
// Brought traits into scope to fix the 'function or associated item not found' errors
use mlua::{FromLua, Function, IntoLua, Lua, RegistryKey, Table};

#[derive(Debug)]
pub enum TranslateError {
    Io(std::io::Error),
    Lua(mlua::Error),
    Missing(String),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Lua(e) => write!(f, "lua: {e}"),
            Self::Missing(k) => write!(f, "no translator registered for {k:?}"),
        }
    }
}

impl std::error::Error for TranslateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Lua(e) => Some(e),
            Self::Missing(_) => None,
        }
    }
}

impl From<std::io::Error> for TranslateError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<mlua::Error> for TranslateError {
    fn from(e: mlua::Error) -> Self {
        Self::Lua(e)
    }
}

// Safe wrapper for values that might be missing/nil
#[derive(Clone, Copy, Debug)]
struct Value(pub Option<f32>);

// Fixed: Removed the unnecessary `'lua` generic lifetime
impl FromLua for Value {
    fn from_lua(v: mlua::Value, _lua: &Lua) -> mlua::Result<Self> {
        match v {
            mlua::Value::Number(n) => Ok(Value(Some(n as f32))),
            mlua::Value::Integer(i) => Ok(Value(Some(i as f32))),
            mlua::Value::Nil => Ok(Value(None)),
            mlua::Value::UserData(ud) => {
                if let Ok(val) = ud.borrow::<Value>() {
                    Ok(*val)
                } else {
                    Ok(Value(None))
                }
            }
            _ => Ok(Value(None)),
        }
    }
}

// Fixed: Removed the explicit `IntoLua` implementation because `mlua`
// automatically generates it for anything implementing `UserData`.

impl mlua::UserData for Value {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        // Overloading Addition (+)
        methods.add_meta_function(
            mlua::MetaMethod::Add,
            |lua, (a, b): (mlua::Value, mlua::Value)| {
                let va = Value::from_lua(a, lua)?;
                let vb = Value::from_lua(b, lua)?;
                Ok(Value(match (va.0, vb.0) {
                    (Some(x), Some(y)) => Some(x + y),
                    _ => None,
                }))
            },
        );

        // Overloading Subtraction (-)
        methods.add_meta_function(
            mlua::MetaMethod::Sub,
            |lua, (a, b): (mlua::Value, mlua::Value)| {
                let va = Value::from_lua(a, lua)?;
                let vb = Value::from_lua(b, lua)?;
                Ok(Value(match (va.0, vb.0) {
                    (Some(x), Some(y)) => Some(x - y),
                    _ => None,
                }))
            },
        );

        // Overloading Multiplication (*)
        methods.add_meta_function(
            mlua::MetaMethod::Mul,
            |lua, (a, b): (mlua::Value, mlua::Value)| {
                let va = Value::from_lua(a, lua)?;
                let vb = Value::from_lua(b, lua)?;
                Ok(Value(match (va.0, vb.0) {
                    (Some(x), Some(y)) => Some(x * y),
                    _ => None,
                }))
            },
        );

        // Overloading Division (/)
        methods.add_meta_function(
            mlua::MetaMethod::Div,
            |lua, (a, b): (mlua::Value, mlua::Value)| {
                let va = Value::from_lua(a, lua)?;
                let vb = Value::from_lua(b, lua)?;
                Ok(Value(match (va.0, vb.0) {
                    (Some(x), Some(y)) => Some(x / y),
                    _ => None,
                }))
            },
        );

        // Overloading Unary Minus (-x)
        methods.add_meta_function(mlua::MetaMethod::Unm, |lua, a: mlua::Value| {
            let va = Value::from_lua(a, lua)?;
            Ok(Value(va.0.map(|x| -x)))
        });
    }
}

pub struct Translator {
    lua: Lua,
    functions: HashMap<String, RegistryKey>,
}

impl Translator {
    pub fn from_scripts<'a>(
        scripts: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, TranslateError> {
        let lua = Lua::new();

        // Shadow standard math functions to play nicely with our UserData `Value`
        if let Ok(math) = lua.globals().get::<Table>("math") {
            // 1. Absolute Value
            let custom_abs = lua.create_function(|lua, v: mlua::Value| {
                let val = Value::from_lua(v, lua)?;
                Ok(Value(val.0.map(|x| x.abs())))
            })?;
            math.set("abs", custom_abs)?;

            // 2. Maximum (Supports variable arguments)
            let custom_max = lua.create_function(|lua, args: mlua::Variadic<mlua::Value>| {
                let mut current_max: Option<f32> = None;

                for arg in args {
                    let val = Value::from_lua(arg, lua)?;
                    match val.0 {
                        None => return Ok(Value(None)), // Short-circuit: if ANY value is nil, max is nil
                        Some(x) => {
                            current_max = Some(match current_max {
                                Some(m) => m.max(x),
                                None => x, // First element initialization
                            });
                        }
                    }
                }
                Ok(Value(current_max))
            })?;
            math.set("max", custom_max)?;

            // 3. Minimum (Supports variable arguments)
            let custom_min = lua.create_function(|lua, args: mlua::Variadic<mlua::Value>| {
                let mut current_min: Option<f32> = None;

                for arg in args {
                    let val = Value::from_lua(arg, lua)?;
                    match val.0 {
                        None => return Ok(Value(None)), // Short-circuit: if ANY value is nil, min is nil
                        Some(x) => {
                            current_min = Some(match current_min {
                                Some(m) => m.min(x),
                                None => x, // First element initialization
                            });
                        }
                    }
                }
                Ok(Value(current_min))
            })?;
            math.set("min", custom_min)?;
        }

        let mut functions = HashMap::new();
        for (name, source) in scripts {
            let func: Function = lua.load(source).eval()?;
            let key = lua.create_registry_value(func)?;
            functions.insert(name.to_owned(), key);
        }

        Ok(Self { lua, functions })
    }

    pub fn translate<K, I, O>(
        &self,
        module_type: &str,
        raw: &HashMap<K, I>,
    ) -> Result<HashMap<String, O>, TranslateError>
    where
        K: AsRef<str>,
        I: Clone + mlua::IntoLua, // Fixed: Cleaned up lifetimeless bounds
        O: mlua::FromLua,         // Fixed: Cleaned up lifetimeless bounds
    {
        let key = self
            .functions
            .get(module_type)
            .ok_or_else(|| TranslateError::Missing(module_type.to_owned()))?;

        let func: Function = self.lua.registry_value(key)?;

        // Build the `p` table: wrap existing inputs inside Value(Some(x))
        let params: Table = self.lua.create_table()?;
        for (name, value) in raw {
            let lua_val = value.clone().into_lua(&self.lua)?;
            let wrapped = Value::from_lua(lua_val, &self.lua)?;
            params.set(name.as_ref(), wrapped)?;
        }

        // Attach fallback metatable: missing keys return Value(None) instead of standard nil
        let mt = self.lua.create_table()?;
        mt.set(
            "__index",
            self.lua
                .create_function(|_, (_table, _key): (Table, mlua::Value)| Ok(Value(None)))?,
        )?;
        params.set_metatable(Some(mt));

        // Call the translator function
        let result: Table = func.call(params)?;

        // Unpack the Value wrappers back to primitive numbers, ignoring None/Nil entries
        let processed_result: Table = self.lua.create_table()?;
        for pair in result.pairs::<String, mlua::Value>() {
            let (k, v) = pair?;
            let unpacked_v = match Value::from_lua(v, &self.lua) {
                Ok(Value(Some(x))) => mlua::Value::Number(x as f64),
                _ => mlua::Value::Nil,
            };
            if !matches!(unpacked_v, mlua::Value::Nil) {
                processed_result.set(k, unpacked_v)?;
            }
        }

        let mut out = HashMap::new();
        for pair in processed_result.pairs::<String, O>() {
            let (k, v) = pair?;
            out.insert(k, v);
        }

        Ok(out)
    }

    pub fn len(&self) -> usize {
        self.functions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOILER_LUA: &str = r#"
        return function(p)
            return {
                degrees_f32_set_water_temp = p.p_367 * math.abs(p.p_363 - 1) + p.p_3 * p.p_363,
                degrees_f32_room_temp = p.p_9,
                degrees_f32_heating_start_ambient_temp = p.p_341 + p.p_320,
            }
        end
    "#;

    #[test]
    fn test_basic_translate() {
        let t = Translator::from_scripts([("boiler", BOILER_LUA)]).unwrap();

        let raw: HashMap<String, f32> = [
            ("p_367", 2.0_f32),
            ("p_363", 0.0_f32),
            ("p_3", 5.0_f32),
            ("p_9", 21.5_f32),
            ("p_341", 10.0_f32),
            ("p_320", 5.5_f32),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();

        let out: HashMap<String, f32> = t.translate("boiler", &raw).unwrap();

        assert!((out["degrees_f32_set_water_temp"] - 2.0).abs() < 1e-5);
        assert!((out["degrees_f32_room_temp"] - 21.5).abs() < 1e-5);
        assert!((out["degrees_f32_heating_start_ambient_temp"] - 15.5).abs() < 1e-5);
    }

    #[test]
    fn test_missing_values_propagate_nil() {
        let t = Translator::from_scripts([("boiler", BOILER_LUA)]).unwrap();

        let raw: HashMap<String, f32> = [
            ("p_367", 2.0_f32),
            ("p_363", 0.0_f32),
            ("p_3", 5.0_f32),
            ("p_9", 21.5_f32),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();

        let out: HashMap<String, f32> = t.translate("boiler", &raw).unwrap();

        assert!((out["degrees_f32_set_water_temp"] - 2.0).abs() < 1e-5);
        assert!((out["degrees_f32_room_temp"] - 21.5).abs() < 1e-5);

        // Missing values evaluated to Value(None), translating cleanly out of the map
        assert!(!out.contains_key("degrees_f32_heating_start_ambient_temp"));

        let raw: HashMap<String, Option<f32>> = [
            ("p_367", Some(2.0_f32)),
            ("p_363", Some(0.0_f32)),
            ("p_3", Some(5.0_f32)),
            ("p_9", Some(21.5_f32)),
            ("p_341", None),
            ("p_320", None),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();

        let out: HashMap<String, f32> = t.translate("boiler", &raw).unwrap();

        assert!((out["degrees_f32_set_water_temp"] - 2.0).abs() < 1e-5);
        assert!((out["degrees_f32_room_temp"] - 21.5).abs() < 1e-5);

        // Missing values evaluated to Value(None), translating cleanly out of the map
        assert!(!out.contains_key("degrees_f32_heating_start_ambient_temp"));
    }

    #[test]
    fn test_missing_module_type() {
        let t = Translator::from_scripts([("boiler", BOILER_LUA)]).unwrap();
        let raw: HashMap<String, f32> = HashMap::new();
        assert!(matches!(
            t.translate::<String, f32, f32>("unknown", &raw),
            Err(TranslateError::Missing(_))
        ));
    }

    #[test]
    fn test_custom_math_functions() {
        const MATH_TEST_LUA: &str = r#"
            return function(p)
                return {
                    abs_positive = math.abs(p.pos),
                    abs_negative = math.abs(p.neg),
                    
                    max_valid = math.max(p.a, p.b, p.c),
                    min_valid = math.min(p.a, p.b, p.c),
                    
                    max_with_nil = math.max(p.a, p.missing_key, p.b),
                    min_with_nil = math.min(p.c, p.missing_key),
                }
            end
        "#;
        // Initialize the translator with our test script
        let t = Translator::from_scripts([("math_checks", MATH_TEST_LUA)]).unwrap();

        // Prepare raw input mapping
        let raw: HashMap<String, f32> = [
            ("pos", 4.2_f32),
            ("neg", -9.5_f32),
            ("a", 10.0_f32),
            ("b", 45.5_f32),
            ("c", -12.0_f32),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();

        // Execute translation
        let out: HashMap<String, f32> = t.translate("math_checks", &raw).unwrap();

        // 1. Verify math.abs behaves correctly
        assert!((out["abs_positive"] - 4.2).abs() < 1e-5);
        assert!((out["abs_negative"] - 9.5).abs() < 1e-5);

        // 2. Verify variadic math.max and math.min work with all values present
        assert!((out["max_valid"] - 45.5).abs() < 1e-5); // 45.5 is the highest
        assert!((out["min_valid"] - (-12.0)).abs() < 1e-5); // -12.0 is the lowest

        // 3. Verify that if ANY parameter is nil, the entry is completely omitted
        // from the final Map (due to Value(None) matching mlua::Value::Nil)
        assert!(
            !out.contains_key("max_with_nil"),
            "max_with_nil should have been omitted due to nil propagation"
        );
        assert!(
            !out.contains_key("min_with_nil"),
            "min_with_nil should have been omitted due to nil propagation"
        );
    }
}
