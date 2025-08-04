use crate::Result;
use std::collections::BTreeMap;
use std::fmt::Formatter;
use std::str::FromStr;

use serde::{Deserialize, de};

use crate::config::config_file::mise_toml::EnvList;

pub struct TomlParser<'a> {
    table: &'a toml::Value,
}

impl<'a> TomlParser<'a> {
    pub fn new(table: &'a toml::Value) -> Self {
        Self { table }
    }

    pub fn parse_str<T>(&self, key: &str) -> Option<T>
    where
        T: From<String>,
    {
        self.table
            .get(key)
            .and_then(|value| value.as_str())
            .map(|value| value.to_string().into())
    }
    pub fn parse_bool(&self, key: &str) -> Option<bool> {
        self.table.get(key).and_then(|value| value.as_bool())
    }
    pub fn parse_array<T>(&self, key: &str) -> Option<Vec<T>>
    where
        T: From<String>,
    {
        self.table
            .get(key)
            .and_then(|value| value.as_array())
            .map(|array| {
                array
                    .iter()
                    .filter_map(|value| value.as_str().map(|v| v.to_string().into()))
                    .collect::<Vec<T>>()
            })
    }
    pub fn parse_table(&self, key: &str) -> Option<BTreeMap<String, toml::Value>> {
        self.table
            .get(key)
            .and_then(|value| value.as_table())
            .map(|table| {
                table
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<BTreeMap<String, toml::Value>>()
            })
    }

    pub fn parse_env(&self, key: &str) -> Result<Option<EnvList>> {
        self.table
            .get(key)
            .map(|value| {
                EnvList::deserialize(value.clone())
                    .map_err(|e| eyre::eyre!("failed to parse env: {}", e))
            })
            .transpose()
    }
}

pub fn deserialize_arr<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: de::Deserializer<'de>,
    T: FromStr + Deserialize<'de>,
    <T as FromStr>::Err: std::fmt::Display,
{
    struct ArrVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T> de::Visitor<'de> for ArrVisitor<T>
    where
        T: FromStr + Deserialize<'de>,
        <T as FromStr>::Err: std::fmt::Display,
    {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("string or array of strings")
        }

        fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            let v = v.parse().map_err(de::Error::custom)?;
            Ok(vec![v])
        }

        fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Self::Value, S::Error>
        where
            S: de::SeqAccess<'de>,
        {
            let mut v = vec![];
            while let Some(s) = seq.next_element::<String>()? {
                v.push(s.parse().map_err(de::Error::custom)?);
            }
            Ok(v)
        }

        fn visit_map<M>(self, map: M) -> std::result::Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            Ok(vec![Deserialize::deserialize(
                de::value::MapAccessDeserializer::new(map),
            )?])
        }
    }

    deserializer.deserialize_any(ArrVisitor(std::marker::PhantomData))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_arr() {
        let toml = r#"arr = ["1", "2", "3"]"#;
        let table = toml::from_str(toml).unwrap();
        let parser = TomlParser::new(&table);
        let arr = parser.parse_array::<String>("arr");
        assert_eq!(arr.unwrap().join(":"), "1:2:3");
    }

    #[test]
    fn test_parse_table() {
        let toml = r#"table = {foo = "bar", baz = "qux", num = 123}"#;
        let table = toml::from_str(toml).unwrap();
        let parser = TomlParser::new(&table);
        let table = parser.parse_table("table").unwrap();
        assert_eq!(table.len(), 3);
        assert_eq!(table.get("foo").unwrap().as_str().unwrap(), "bar");
        assert_eq!(table.get("baz").unwrap().as_str().unwrap(), "qux");
        assert_eq!(table.get("num").unwrap().as_integer().unwrap(), 123);
    }
}
