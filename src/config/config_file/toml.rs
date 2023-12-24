use std::collections::HashMap;

pub struct TomlParser<'a> {
    pub table: &'a toml::Value,
}

impl<'a> TomlParser<'a> {
    pub fn new(table: &'a toml::Value) -> Self {
        Self { table }
    }

    pub fn parse_str(&self, key: &str) -> Option<String> {
        self.table
            .get(key)
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
    }
    pub fn parse_bool(&self, key: &str) -> Option<bool> {
        self.table.get(key).and_then(|value| value.as_bool())
    }
    pub fn parse_array<T>(&self, key: &str) -> Option<Vec<T>>
    where
        T: Default + From<String>,
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
    pub fn parse_hashmap<T>(&self, key: &str) -> Option<HashMap<String, T>>
    where
        T: From<String>,
    {
        self.table
            .get(key)
            .and_then(|value| value.as_table())
            .map(|table| {
                table
                    .iter()
                    .filter_map(|(key, value)| {
                        value
                            .as_str()
                            .map(|v| (key.to_string(), v.to_string().into()))
                    })
                    .collect::<HashMap<String, T>>()
            })
    }
}
