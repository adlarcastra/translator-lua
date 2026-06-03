use mlua::{FromLua, Function, Lua, RegistryKey, Table};
use std::collections::HashMap;

#[derive(Debug)]
pub enum TranslateError {
    Lua(mlua::Error),
    Missing(String),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lua(e) => write!(f, "lua: {e}"),
            Self::Missing(k) => write!(f, "no translator registered for {k:?}"),
        }
    }
}

impl std::error::Error for TranslateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lua(e) => Some(e),
            Self::Missing(_) => None,
        }
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

        methods.add_meta_method(
            mlua::MetaMethod::Lt,
            |_, this: &Value, other: Value| match (this.0, other.0) {
                (Some(a), Some(b)) => Ok(a < b),
                _ => Ok(false),
            },
        );

        methods.add_meta_method(
            mlua::MetaMethod::Le,
            |_, this: &Value, other: Value| match (this.0, other.0) {
                (Some(a), Some(b)) => Ok(a <= b),
                _ => Ok(false),
            },
        );

        methods.add_meta_method(mlua::MetaMethod::Eq, |_, this: &Value, other: Value| {
            Ok(this.0 == other.0)
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
        I: Clone + mlua::IntoLua,
        O: mlua::FromLua,
    {
        let key = self
            .functions
            .get(module_type)
            .ok_or_else(|| TranslateError::Missing(module_type.to_owned()))?;

        let func: Function = self.lua.registry_value(key)?;

        let params: Table = self.lua.create_table()?;

        for (name, value) in raw {
            let lua_val = value.clone().into_lua(&self.lua)?;
            let wrapped = Value::from_lua(lua_val, &self.lua)?;
            params.set(name.as_ref(), wrapped)?;
        }

        let mt = self.lua.create_table()?;
        mt.set(
            "__index",
            self.lua
                .create_function(|_, (_table, _key): (Table, mlua::Value)| Ok(Value(None)))?,
        )?;
        params.set_metatable(Some(mt))?;

        let result: Table = func.call(params)?;

        let mut out = HashMap::new();

        for pair in result.pairs::<String, mlua::Value>() {
            let (k, v) = pair?;

            match Value::from_lua(v, &self.lua)? {
                Value(Some(x)) => {
                    let value = O::from_lua(mlua::Value::Number(x as f64), &self.lua)?;
                    out.insert(k, value);
                }
                Value(None) => {
                    if let Ok(value) = O::from_lua(mlua::Value::Nil, &self.lua) {
                        out.insert(k, value);
                    }
                }
            }
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
    use rand::RngExt;

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

        let out: HashMap<String, Option<f32>> = t.translate("boiler", &raw).unwrap();

        assert!(out.contains_key("degrees_f32_heating_start_ambient_temp"));
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

    #[test]
    fn test_real_translation() {
        const REAL_LUA: &str = r#"
            return function(p)
                return {
                    degrees_f32_set_water_temp = p.p_367 * math.abs(p.p_363 - 1) + p.p_3 * p.p_363,

                    degrees_f32_room_temp = p.p_9,
                    degrees_f32_desired_temp = p.p_10,

                    degrees_f32_outside_temp = p.p_76 / 10,
                    degrees_f32_supply_temp = p.p_82 / 10,
                    degrees_f32_return_temp = p.p_81 / 10,
                    lmin_f32_water_flow = p.p_90 / 10,

                    degrees_f32_heating_start_ambient_temp = p.p_280,

                    onoff_f32_power_on = p.p_775,
                    enum_f32_heating_mode = p.p_774,

                    hz_f32_compressor_frequency = p.p_66,
                    hz_f32_fan_frequency = p.p_67 / 60,


                    onoff_f32_sensor_enable_boiler = p.p_306,
                    onoff_f32_linkage_switch_setting = p.p_263,
                    lmin_f32_flow_value_during_e03 = p.p_392,
                    degrees_f32_temp_difference_for_e37 = p.p_281,

                    degrees_f32_manual_set_temp_heating = p.p_771,
                    degrees_f32_manual_set_temp_cooling = p.p_770,

                    enum_f32_heating_curve_heating_select = p.p_790,
                    enum_f32_heating_curve_cooling_select = p.p_789,

                    degrees_f32_max_outside_temp_heating = p.p_364,
                    degrees_f32_min_outside_temp_cooling = p.p_363,

                    degrees_f32_max_water_temp_heating = p.p_368,
                    degrees_f32_min_water_temp_heating = p.p_369,

                    degrees_f32_min_water_temp_cooling = p.p_371,
                    degrees_f32_max_water_temp_cooling = p.p_372,

                    onoff_f32_custom_heating_curve_heating = p.p_2066,
                    slope_f32_custom_heating_curve_heating_slope = p.p_2067,
                    degrees_f32_custom_heating_curve_heating_start = p.p_2068,

                    onoff_f32_custom_heating_curve_cooling = p.p_2095,
                    slope_f32_custom_heating_curve_cooling_slope = p.p_2096,
                    degrees_f32_custom_heating_curve_cooling_start = p.p_2097,

                    degrees_f32_hysteresis_a = p.p_284,

                    enum_f32_circulation_pump_mode = p.p_286,
                    onoff_f32_flow_switch = p.p_261,

                    percentage_f32_circulation_pump_max_speed = p.p_518,
                    percentage_f32_circulation_pump_min_speed = p.p_358,
                    percentage_f32_circulation_pump_min_speed_regulation = p.p_421,
                    percentage_f32_circulation_pump_speed_at_set_temp = p.p_519,

                    degrees_f32_temp_difference_for_circulation_pump_activation = p.p_357,

                    onoff_f32_heatpump_control_based_on_inlet = p.p_374,

                    onoff_f32_anti_block_circulation_on = p.p_2112,
                    days_f32_anti_block_circulation_interval = p.p_2113,

                    enum_f32_defrosting_mode = p.p_288,

                    rpm_f32_max_fan_frequency_heating = p.p_326,
                    rpm_f32_max_fan_frequency_cooling = p.p_328,
                    rpm_f32_max_fan_frequency_silent = p.p_347,

                    slope_f32_night_curve_slope = p.p_2076,
                    degrees_f32_night_curve_start = p.p_2077,
                    degrees_f32_night_curve_start_time = p.p_2078,
                    degrees_f32_night_curve_end_time = p.p_2079,

                    x_f32_current_unit_tool_number = p.p_38,
                    x_f32_eev_open_step = p.p_68,
                    x_f32_evi_valve_open_step = p.p_69,

                    volt_f32_ac_input_voltage = p.p_70,

                    amp_f32_ac_input_current = p.p_71 / 10,
                    amp_f32_compressor_phase_current = p.p_72 / 10,

                    degrees_f32_compressor_ipm_temp = p.p_73,
                    degrees_f32_high_pressure_saturation_temp = p.p_74,
                    degrees_f32_low_pressure_saturation_temp = p.p_75,

                    degrees_f32_outer_coil_temp = p.p_77,
                    degrees_f32_coil_temp = p.p_78,
                    degrees_f32_suction_temp = p.p_79,
                    degrees_f32_exhaust_temp = p.p_80,

                    degrees_f32_economizer_inlet_temp = p.p_83,
                    degrees_f32_economizer_outlet_temp = p.p_84,

                    degrees_f32_plate_heat_exchanger_exhaust_temp = p.p_87,

                    x_f32_water_pump_speed_pwm = p.p_89,

                    volt_f32_unit_input_voltage = p.p_92,

                    amp_f32_unit_input_current = p.p_93 / 10,
                    kw_f32_unit_input_power = p.p_94 / 10,

                    kwh_f32_unit_input_power_consumption = p.p_95,

                    x_f32_smart_grid_status = p.p_124,
                    x_f32_number = p.p_226,

                    kwh_f32_power_consumption_6_min = p.p_227,
                    x_f32_cooling_capacity_6_min = p.p_228,
                    x_f32_eer_6_min = p.p_229,
                    x_f32_current_eer = p.p_230,

                    x_f32_heating_capacity_6_min = p.p_235,
                    x_f32_cop_6_min = p.p_236,
                    x_f32_current_cop = p.p_237,

                    degrees_f32_set_dhw_temp = p.p_772 / 10,

                    kwh_f32_program_version = p.p_866,

                    onoff_f32_sterilization = p.p_2051,
                    days_f32_sterilization_interval = p.p_2052,
                    time_f32_sterilization_start_time = p.p_2053,
                    minutes_f32_sterilization_run_time = p.p_2054,
                    degrees_f32_sterilization_temp = p.p_2055 / 10,

                    degrees_f32_dhw_tank_temp = p.p_86 / 10,

                    x_f32_water_pump_pwm_range_setting = p.p_404,
                }
            end
        "#;
        // Initialize the translator with our test script
        let mut rng = rand::rng();

        for _ in 0..1000 {
            let mut p: HashMap<String, Option<f32>> = HashMap::with_capacity(10_001);
            let t = Translator::from_scripts([("real", REAL_LUA)]).unwrap();
            for i in 0..=10_000 {
                let value = if rng.random_bool(0.1) {
                    None // 10% missing
                } else {
                    Some(rng.random_range(0.0..1000.0))
                };

                p.insert(format!("p_{i}"), value);
            }
            let _: HashMap<String, f32> = t.translate("real", &p).unwrap();
        }
    }
}
