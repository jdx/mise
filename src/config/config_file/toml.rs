use crate::Result;
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::config::config_file::mise_toml::EnvList;

pub(crate) struct TomlParser<'a> {
    table: &'a toml::Value,
}

impl<'a> TomlParser<'a> {
    pub(crate) fn new(table: &'a toml::Value) -> Self {
        Self { table }
    }

    pub(crate) fn parse_str<T>(&self, key: &str) -> Option<T>
    where
        T: From<String>,
    {
        self.table
            .get(key)
            .and_then(|value| value.as_str())
            .map(|value| value.to_string().into())
    }
    pub(crate) fn parse_bool(&self, key: &str) -> Option<bool> {
        self.table.get(key).and_then(|value| value.as_bool())
    }
    pub(crate) fn parse_array<T>(&self, key: &str) -> Option<Vec<T>>
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
    pub(crate) fn parse_table(&self, key: &str) -> Option<BTreeMap<String, toml::Value>> {
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

    pub(crate) fn parse_env(&self, key: &str) -> Result<Option<EnvList>> {
        self.table
            .get(key)
            .map(|value| {
                EnvList::deserialize(value.clone())
                    .map_err(|e| eyre::eyre!("failed to parse env: {}", e))
            })
            .transpose()
    }
}

pub(crate) struct TrackingTomlParser<'a> {
    inner: TomlParser<'a>,
    table: &'a toml::Value,
    parsed_keys: std::collections::BTreeSet<String>,
}

impl<'a> TrackingTomlParser<'a> {
    pub(crate) fn new(table: &'a toml::Value) -> Self {
        Self {
            inner: TomlParser::new(table),
            table,
            parsed_keys: std::collections::BTreeSet::new(),
        }
    }

    fn record(&mut self, key: &str) {
        self.parsed_keys.insert(key.to_string());
    }

    #[cfg(test)]
    pub(crate) fn parsed_keys(&self) -> impl Iterator<Item = &str> {
        self.parsed_keys.iter().map(|s| s.as_str())
    }

    pub(crate) fn unparsed_keys(&self) -> Vec<String> {
        if let Some(table) = self.table.as_table() {
            table
                .keys()
                .filter(|k| !self.parsed_keys.contains(k.as_str()))
                .cloned()
                .collect()
        } else {
            vec![]
        }
    }

    pub(crate) fn parse_str<T>(&mut self, key: &str) -> Option<T>
    where
        T: From<String>,
    {
        self.record(key);
        self.inner.parse_str::<T>(key)
    }

    pub(crate) fn parse_bool(&mut self, key: &str) -> Option<bool> {
        self.record(key);
        self.inner.parse_bool(key)
    }

    pub(crate) fn parse_array<T>(&mut self, key: &str) -> Option<Vec<T>>
    where
        T: From<String>,
    {
        self.record(key);
        self.inner.parse_array::<T>(key)
    }

    pub(crate) fn parse_table(&mut self, key: &str) -> Option<BTreeMap<String, toml::Value>> {
        self.record(key);
        self.inner.parse_table(key)
    }

    pub(crate) fn parse_env(&mut self, key: &str) -> Result<Option<EnvList>> {
        self.record(key);
        self.inner.parse_env(key)
    }

    pub(crate) fn get_raw(&mut self, key: &str) -> Option<&'a toml::Value> {
        self.record(key);
        self.table.get(key)
    }
}

pub(crate) use mise_config::de::deserialize_arr;

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
    fn test_parse_env_requires_table() {
        let table = toml::from_str(r#"env = [{ FOO = "bar" }]"#).unwrap();
        assert!(TomlParser::new(&table).parse_env("env").is_err());

        let table = toml::from_str(
            r#"
            [env]
            _.source = ["./first.sh", "./second.sh"]
            "#,
        )
        .unwrap();
        assert!(TomlParser::new(&table).parse_env("env").is_ok());
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
