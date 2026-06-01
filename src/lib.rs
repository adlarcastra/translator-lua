use std::{collections::HashMap, path::Path};

use mlua::{Function, Lua, RegistryKey, Table};

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

pub struct Translator {
    lua: Lua,
    functions: HashMap<String, RegistryKey>,
}

impl Translator {
    pub fn from_scripts<'a>(
        scripts: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, TranslateError> {
        let lua = Lua::new();
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
        I: Clone + mlua::IntoLua,
        O: mlua::FromLua,
    {
        let key = self
            .functions
            .get(module_type)
            .ok_or_else(|| TranslateError::Missing(module_type.to_owned()))?;

        let func: Function = self.lua.registry_value(key)?;

        // Build the `p` table: p.register_name = value
        let params: Table = self.lua.create_table()?;
        for (name, value) in raw {
            params.set(name.as_ref(), value.clone())?;
        }

        // Call the translator function
        let result: Table = func.call(params)?;

        let mut out = HashMap::new();
        for pair in result.pairs::<String, O>() {
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
            }
        end
    "#;

    #[test]
    fn test_basic_translate() {
        let t = Translator::from_scripts([("boiler", BOILER_LUA)]).unwrap();

        let raw: HashMap<String, f32> = [
            ("p_367", 2.0_f32),
            ("p_363", 0.0_f32), // abs(0 - 1) = 1
            ("p_3", 5.0_f32),
            ("p_9", 21.5_f32),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();

        let out: HashMap<String, f32> = t.translate("boiler", &raw).unwrap();
        // p_367 * abs(p_363 - 1) + p_3 * p_363
        // = 2.0 * abs(0.0 - 1.0) + 5.0 * 0.0
        // = 2.0 * 1.0 + 0.0 = 2.0
        assert!((out["degrees_f32_set_water_temp"] - 2.0).abs() < 1e-5);
        assert!((out["degrees_f32_room_temp"] - 21.5).abs() < 1e-5);
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
}
