use std::collections::HashMap;

use tera::{Context, Tera};

pub struct TomlParser<'a> {
    table: &'a toml::Value,
    tera: Tera,
    tera_ctx: Context,
}

impl<'a> TomlParser<'a> {
    pub fn new(table: &'a toml::Value, tera: Tera, tera_ctx: Context) -> Self {
        Self {
            table,
            tera,
            tera_ctx,
        }
    }

    pub fn parse_str<T>(&self, key: &str) -> eyre::Result<Option<T>>
    where
        T: From<String>,
    {
        self.table
            .get(key)
            .and_then(|value| value.as_str())
            .map(|s| self.render_tmpl(s))
            .transpose()
    }
    pub fn parse_bool(&self, key: &str) -> Option<bool> {
        self.table.get(key).and_then(|value| value.as_bool())
    }
    pub fn parse_array<T>(&self, key: &str) -> eyre::Result<Option<Vec<T>>>
    where
        T: Default + From<String>,
    {
        self.table
            .get(key)
            .and_then(|value| value.as_array())
            .map(|array| {
                array
                    .iter()
                    .filter_map(|value| value.as_str().map(|v| self.render_tmpl(v)))
                    .collect::<eyre::Result<Vec<T>>>()
            })
            .transpose()
    }
    pub fn parse_hashmap<T>(&self, key: &str) -> eyre::Result<Option<HashMap<String, T>>>
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
                            .map(|v| Ok((self.render_tmpl(key)?, self.render_tmpl(v)?)))
                    })
                    .collect::<eyre::Result<HashMap<String, T>>>()
            })
            .transpose()
    }

    fn render_tmpl<T>(&self, tmpl: &str) -> eyre::Result<T>
    where
        T: From<String>,
    {
        let tmpl = self.tera.clone().render_str(tmpl, &self.tera_ctx)?;
        Ok(tmpl.into())
    }
}
