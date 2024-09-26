use std::fmt::Debug;
use std::collections::BTreeMap;
use std::fmt::Formatter;
use std::str::FromStr;

use either::Either;
use serde::de;
use tera::{Context, Tera};

use crate::task::EitherStringOrBool;

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
    pub fn parse_env(
        &self,
        key: &str,
    ) -> eyre::Result<Option<BTreeMap<String, EitherStringOrBool>>> {
        self.table
            .get(key)
            .and_then(|value| value.as_table())
            .map(|table| {
                table
                    .iter()
                    .map(|(key, value)| {
                        let v = value
                            .as_str()
                            .map(|v| Ok(EitherStringOrBool(Either::Left(self.render_tmpl(v)?))))
                            .or_else(|| {
                                value
                                    .as_integer()
                                    .map(|v| Ok(EitherStringOrBool(Either::Left(v.to_string()))))
                            })
                            .or_else(|| {
                                value
                                    .as_bool()
                                    .map(|v| Ok(EitherStringOrBool(Either::Right(v))))
                            })
                            .unwrap_or_else(|| {
                                Err(eyre::eyre!("invalid env value: {:?}", value))
                            })?;
                        Ok((key.clone(), v))
                    })
                    .collect::<eyre::Result<_>>()
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

pub fn deserialize_arr<'de, D, T>(deserializer: D) -> eyre::Result<Vec<T>, D::Error>
where
    D: de::Deserializer<'de>,
    T: FromStr,
    <T as FromStr>::Err: std::fmt::Display,
{
    struct ArrVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T> de::Visitor<'de> for ArrVisitor<T>
    where
        T: FromStr,
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
    }

    deserializer.deserialize_any(ArrVisitor(std::marker::PhantomData))
}

pub fn deserialize_path_entry_arr<'de, D, T>(deserializer: D) -> eyre::Result<Vec<T>, D::Error>
where
    D: de::Deserializer<'de>,
    T: FromStr + Debug + serde::Deserialize<'de>,
    <T as FromStr>::Err: std::fmt::Display,
{
    struct PathEntryArrVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T> de::Visitor<'de> for PathEntryArrVisitor<T>
    where
        T: FromStr + Debug + serde::Deserialize<'de>,
        <T as FromStr>::Err: std::fmt::Display,
    {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("path entry or array of path entries")
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
            while let Some(entry) = seq.next_element::<T>()? {
                trace!("visit_seq: entry: {:?}", entry);
                v.push(entry);
            }
            Ok(v)
        }
    }

    deserializer.deserialize_any(PathEntryArrVisitor(std::marker::PhantomData))
}
