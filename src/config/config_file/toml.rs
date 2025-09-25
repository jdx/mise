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

pub fn deserialize_arr<'de, D, C, T>(deserializer: D) -> std::result::Result<C, D::Error>
where
    D: de::Deserializer<'de>,
    C: FromIterator<T> + Deserialize<'de>,
    T: FromStr + Deserialize<'de>,
    <T as FromStr>::Err: std::fmt::Display,
{
    struct ArrVisitor<C, T>(std::marker::PhantomData<(C, T)>);

    impl<'de, C, T> de::Visitor<'de> for ArrVisitor<C, T>
    where
        C: FromIterator<T> + Deserialize<'de>,
        T: FromStr + Deserialize<'de>,
        <T as FromStr>::Err: std::fmt::Display,
    {
        type Value = C;
        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("a string, a map, or a list of strings/maps")
        }

        fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            let v = v.parse().map_err(de::Error::custom)?;
            Ok(std::iter::once(v).collect())
        }

        fn visit_map<M>(self, map: M) -> std::result::Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            let item = T::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(std::iter::once(item).collect())
        }

        fn visit_seq<S>(self, seq: S) -> std::result::Result<Self::Value, S::Error>
        where
            S: de::SeqAccess<'de>,
        {
            #[derive(Deserialize)]
            #[serde(untagged)]
            enum StringOrValue<T> {
                String(String),
                Value(T),
            }
            let mut seq = seq;
            std::iter::from_fn(|| seq.next_element::<StringOrValue<T>>().transpose())
                .map(|element| match element {
                    Ok(StringOrValue::String(s)) => s.parse().map_err(de::Error::custom),
                    Ok(StringOrValue::Value(v)) => Ok(v),
                    Err(e) => Err(e),
                })
                .collect()
        }
    }

    deserializer.deserialize_any(ArrVisitor(std::marker::PhantomData))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::str::FromStr;

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

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    #[serde(untagged)]
    enum TestItem {
        String(String),
        Object { a: String, b: i64 },
    }

    impl FromStr for TestItem {
        type Err = String;

        fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
            Ok(TestItem::String(s.to_string()))
        }
    }

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestStruct {
        #[serde(default, deserialize_with = "deserialize_arr")]
        arr: Vec<TestItem>,
    }

    #[test]
    fn test_deserialize_arr_string() {
        let toml_str = r#"arr = "hello""#;
        let expected = TestStruct {
            arr: vec![TestItem::String("hello".to_string())],
        };
        let actual: TestStruct = toml::from_str(toml_str).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_arr_string_list() {
        let toml_str = r#"arr = ["hello", "world"]"#;
        let expected = TestStruct {
            arr: vec![
                TestItem::String("hello".to_string()),
                TestItem::String("world".to_string()),
            ],
        };
        let actual: TestStruct = toml::from_str(toml_str).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_arr_map() {
        let toml_str = r#"arr = { a = "foo", b = 123 }"#;
        let expected = TestStruct {
            arr: vec![TestItem::Object {
                a: "foo".to_string(),
                b: 123,
            }],
        };
        let actual: TestStruct = toml::from_str(toml_str).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_arr_map_list() {
        let toml_str = r#"
        arr = [
            { a = "foo", b = 123 },
            { a = "bar", b = 456 },
        ]
        "#;
        let expected = TestStruct {
            arr: vec![
                TestItem::Object {
                    a: "foo".to_string(),
                    b: 123,
                },
                TestItem::Object {
                    a: "bar".to_string(),
                    b: 456,
                },
            ],
        };
        let actual: TestStruct = toml::from_str(toml_str).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_arr_mixed_list() {
        let toml_str = r#"
        arr = [
            "hello",
            { a = "foo", b = 123 },
        ]
        "#;
        let expected = TestStruct {
            arr: vec![
                TestItem::String("hello".to_string()),
                TestItem::Object {
                    a: "foo".to_string(),
                    b: 123,
                },
            ],
        };
        let actual: TestStruct = toml::from_str(toml_str).unwrap();
        assert_eq!(actual, expected);
    }
}
