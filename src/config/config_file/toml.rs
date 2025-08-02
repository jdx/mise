use std::collections::BTreeMap;
use std::fmt::Formatter;
use std::str::FromStr;

use serde::{Deserialize, de};

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
            let mut index = 0;
            while let Some(element) = seq.next_element::<toml::Value>()? {
                match element.as_str() {
                    Some(s) => {
                        v.push(s.parse().map_err(de::Error::custom)?);
                    }
                    None => {
                        return Err(de::Error::custom(format!(
                            "array element at index {index} is not a string: {element:?}"
                        )));
                    }
                }
                index += 1;
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

    #[test]
    fn test_deserialize_arr_valid() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct TestStruct {
            #[serde(deserialize_with = "deserialize_arr")]
            files: Vec<String>,
        }

        let valid_toml = r#"files = ["file1.txt", "file2.txt"]"#;
        let result: TestStruct = toml::from_str(valid_toml).unwrap();
        assert_eq!(result.files, vec!["file1.txt", "file2.txt"]);
    }

    #[test]
    fn test_deserialize_arr_invalid_mixed_types() {
        #[derive(Debug, Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "deserialize_arr")]
            files: Vec<String>,
        }

        let invalid_toml = r#"files = ["file1.txt", 123]"#;
        let result = toml::from_str::<TestStruct>(invalid_toml);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("array element at index 1 is not a string")
        );
        assert!(error.to_string().contains("123"));
    }

    #[test]
    fn test_deserialize_arr_invalid_boolean() {
        #[derive(Debug, Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "deserialize_arr")]
            paths: Vec<String>,
        }

        let invalid_toml = r#"paths = ["./bin", true]"#;
        let result = toml::from_str::<TestStruct>(invalid_toml);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("array element at index 1 is not a string")
        );
        assert!(error.to_string().contains("true"));
    }

    #[test]
    fn test_deserialize_arr_invalid_float() {
        #[derive(Debug, Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "deserialize_arr")]
            sources: Vec<String>,
        }

        let invalid_toml = r#"sources = ["source.sh", 45.6]"#;
        let result = toml::from_str::<TestStruct>(invalid_toml);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("array element at index 1 is not a string")
        );
        assert!(error.to_string().contains("45.6"));
    }
}
