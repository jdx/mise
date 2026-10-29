//! `min_version`: the oldest mise a config works with.

use std::borrow::Cow;
use std::fmt::Formatter;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserializer;
use serde::de::{self, Visitor};
use versions::Versioning;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MinVersionSpec {
    hard: Option<Versioning>,
    soft: Option<Versioning>,
}

impl MinVersionSpec {
    pub fn new(hard: Option<Versioning>, soft: Option<Versioning>) -> Option<Self> {
        if hard.is_none() && soft.is_none() {
            None
        } else {
            Some(Self { hard, soft })
        }
    }

    fn hard(&self) -> Option<&Versioning> {
        self.hard.as_ref()
    }

    fn soft(&self) -> Option<&Versioning> {
        self.soft.as_ref()
    }

    pub fn hard_violation(&self, current: &Versioning) -> Option<&Versioning> {
        self.hard().filter(|required| current < *required)
    }

    pub fn soft_violation(&self, current: &Versioning) -> Option<&Versioning> {
        self.soft().filter(|recommended| current < *recommended)
    }
}

pub fn deserialize_min_version<'de, D>(deserializer: D) -> Result<Option<MinVersionSpec>, D::Error>
where
    D: Deserializer<'de>,
{
    struct MinVersionVisitor;

    impl<'de> Visitor<'de> for MinVersionVisitor {
        type Value = Option<MinVersionSpec>;

        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("string or table for min_version")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(self)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            let version = Versioning::new(v)
                .ok_or_else(|| versions::Error::IllegalVersioning(v.to_string()))
                .map_err(E::custom)?;
            Ok(MinVersionSpec::new(Some(version), None))
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            self.visit_str(&v)
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            let mut hard: Option<Versioning> = None;
            let mut soft: Option<Versioning> = None;
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "hard" => {
                        if hard.is_some() {
                            return Err(de::Error::duplicate_field("hard"));
                        }
                        let value: String = map.next_value()?;
                        let version = Versioning::new(&value)
                            .ok_or_else(|| versions::Error::IllegalVersioning(value.clone()))
                            .map_err(de::Error::custom)?;
                        hard = Some(version);
                    }
                    "soft" => {
                        if soft.is_some() {
                            return Err(de::Error::duplicate_field("soft"));
                        }
                        let value: String = map.next_value()?;
                        let version = Versioning::new(&value)
                            .ok_or_else(|| versions::Error::IllegalVersioning(value.clone()))
                            .map_err(de::Error::custom)?;
                        soft = Some(version);
                    }
                    other => {
                        return Err(de::Error::unknown_field(other, &["hard", "soft"]));
                    }
                }
            }
            Ok(MinVersionSpec::new(hard, soft))
        }
    }

    deserializer.deserialize_option(MinVersionVisitor)
}

// Written by hand next to the parser: a version string, or a table of `hard` and `soft` versions.
impl JsonSchema for MinVersionSpec {
    fn schema_name() -> Cow<'static, str> {
        "min_version".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        crate::schema::hand_written(json_schema!({
            "oneOf": [
                {"pattern": "^\\d+\\.\\d+\\.\\d+$", "type": "string"},
                {
                    "type": "object",
                    "unevaluatedProperties": false,
                    "properties": {
                        "hard": {
                            "description": "error if current mise is lower than this version",
                            "type": "string",
                            "pattern": "^\\d+\\.\\d+\\.\\d+$"
                        },
                        "soft": {
                            "description": "warn if current mise is lower than this version",
                            "type": "string",
                            "pattern": "^\\d+\\.\\d+\\.\\d+$"
                        }
                    }
                }
            ]
        }))
    }
}
