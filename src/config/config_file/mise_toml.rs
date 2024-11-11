use std::collections::{BTreeMap, HashMap};
use std::fmt::{Debug, Formatter};
use std::path::{Path, PathBuf};

use eyre::{eyre, WrapErr};
use indexmap::IndexMap;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use serde::de::Visitor;
use serde::{de, Deserializer};
use serde_derive::Deserialize;
use tera::Context as TeraContext;
use toml_edit::{table, value, Array, DocumentMut, InlineTable, Item, Key, Value};
use versions::Versioning;

use crate::cli::args::{BackendArg, ToolVersionType};
use crate::config::config_file::toml::{deserialize_arr, deserialize_path_entry_arr};
use crate::config::config_file::{trust_check, ConfigFile, TaskConfig};
use crate::config::env_directive::{EnvDirective, PathEntry};
use crate::config::settings::SettingsPartial;
use crate::config::{Alias, AliasMap};
use crate::file::{create_dir_all, display_path};
use crate::registry::REGISTRY_BACKEND_MAP;
use crate::task::Task;
use crate::tera::{get_tera, BASE_CONTEXT};
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource, ToolVersionOptions};
use crate::{dirs, file};

#[derive(Default, Deserialize)]
pub struct MiseToml {
    #[serde(default, deserialize_with = "deserialize_version")]
    min_version: Option<Versioning>,
    #[serde(skip)]
    context: TeraContext,
    #[serde(skip)]
    path: PathBuf,
    #[serde(default, alias = "dotenv", deserialize_with = "deserialize_arr")]
    env_file: Vec<PathBuf>,
    #[serde(default)]
    env: EnvList,
    #[serde(default, deserialize_with = "deserialize_arr")]
    env_path: Vec<PathEntry>,
    #[serde(default)]
    alias: AliasMap,
    #[serde(skip)]
    doc: OnceCell<DocumentMut>,
    #[serde(default)]
    tools: IndexMap<BackendArg, MiseTomlToolList>,
    #[serde(default)]
    plugins: HashMap<String, String>,
    #[serde(default)]
    task_config: TaskConfig,
    #[serde(default)]
    tasks: Tasks,
    #[serde(default)]
    settings: SettingsPartial,
}

#[derive(Debug, Default, Clone)]
pub struct MiseTomlToolList(Vec<MiseTomlTool>);

#[derive(Debug, Clone)]
pub struct MiseTomlTool {
    pub tt: ToolVersionType,
    pub options: Option<ToolVersionOptions>,
}

#[derive(Debug, Default, Clone)]
pub struct Tasks(pub BTreeMap<String, Task>);

#[derive(Debug, Default, Clone)]
pub struct EnvList(pub(crate) Vec<EnvDirective>);

impl MiseToml {
    pub fn init(path: &Path) -> Self {
        let mut context = BASE_CONTEXT.clone();
        context.insert("config_root", path.parent().unwrap().to_str().unwrap());
        Self {
            path: path.to_path_buf(),
            context,
            ..Default::default()
        }
    }

    pub fn from_file(path: &Path) -> eyre::Result<Self> {
        let body = file::read_to_string(path)?;
        Self::from_str(&body, path)
    }

    pub fn from_str(body: &str, path: &Path) -> eyre::Result<Self> {
        trace!("parsing: {}", display_path(path));
        let des = toml::Deserializer::new(body);
        let mut rf: MiseToml = serde_ignored::deserialize(des, |p| {
            warn!("unknown field in {}: {p}", display_path(path));
        })?;
        rf.context = BASE_CONTEXT.clone();
        rf.context
            .insert("config_root", path.parent().unwrap().to_str().unwrap());
        rf.path = path.to_path_buf();
        let project_root = rf.project_root().map(|p| p.to_path_buf());
        for task in rf.tasks.0.values_mut() {
            task.config_source.clone_from(&rf.path);
            task.config_root = project_root.clone();
        }
        // trace!("{}", rf.dump()?);
        Ok(rf)
    }

    fn doc(&self) -> eyre::Result<&DocumentMut> {
        self.doc.get_or_try_init(|| {
            let body = file::read_to_string(&self.path).unwrap_or_default();
            Ok(body.parse()?)
        })
    }

    fn doc_mut(&mut self) -> eyre::Result<&mut DocumentMut> {
        self.doc()?;
        Ok(self.doc.get_mut().unwrap())
    }

    pub fn set_alias(&mut self, fa: &BackendArg, from: &str, to: &str) -> eyre::Result<()> {
        self.alias
            .entry(fa.short.to_string())
            .or_default()
            .versions
            .insert(from.into(), to.into());
        self.doc_mut()?
            .entry("alias")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .entry(&fa.to_string())
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .entry("versions")
            .or_insert_with(table)
            .as_table_like_mut()
            .unwrap()
            .insert(from, value(to));
        Ok(())
    }

    pub fn remove_alias(&mut self, fa: &BackendArg, from: &str) -> eyre::Result<()> {
        if let Some(aliases) = self
            .doc_mut()?
            .get_mut("alias")
            .and_then(|v| v.as_table_mut())
        {
            if let Some(alias) = aliases
                .get_mut(&fa.to_string())
                .and_then(|v| v.as_table_mut())
            {
                if let Some(versions) = alias.get_mut("versions").and_then(|v| v.as_table_mut()) {
                    versions.remove(from);
                    if versions.is_empty() {
                        alias.remove("versions");
                    }
                }
                if alias.is_empty() {
                    aliases.remove(&fa.to_string());
                }
            }
            if aliases.is_empty() {
                self.doc_mut()?.as_table_mut().remove("alias");
            }
        }
        if let Some(aliases) = self.alias.get_mut(&fa.short) {
            aliases.versions.swap_remove(from);
            if aliases.versions.is_empty() && aliases.full.is_none() {
                self.alias.swap_remove(&fa.short);
            }
        }
        Ok(())
    }

    pub fn update_env<V: Into<Value>>(&mut self, key: &str, value: V) -> eyre::Result<()> {
        let env_tbl = self
            .doc_mut()?
            .entry("env")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();
        let key = get_key_with_decor(env_tbl, key);
        env_tbl.insert_formatted(&key, toml_edit::value(value));
        Ok(())
    }

    pub fn remove_env(&mut self, key: &str) -> eyre::Result<()> {
        let env_tbl = self
            .doc_mut()?
            .entry("env")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();
        env_tbl.remove(key);
        Ok(())
    }

    fn parse_template(&self, input: &str) -> eyre::Result<String> {
        if !input.contains("{{") && !input.contains("{%") && !input.contains("{#") {
            return Ok(input.to_string());
        }
        trust_check(&self.path)?;
        let dir = self.path.parent();
        let output = get_tera(dir)
            .render_str(input, &self.context)
            .wrap_err_with(|| {
                let p = display_path(&self.path);
                eyre!("failed to parse template {input} in {p}")
            })?;
        Ok(output)
    }
}

impl ConfigFile for MiseToml {
    fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    fn min_version(&self) -> &Option<Versioning> {
        &self.min_version
    }

    fn project_root(&self) -> Option<&Path> {
        let filename = self.path.file_name().unwrap_or_default().to_string_lossy();
        match self.path.parent() {
            Some(dir) => match dir {
                dir if dir.starts_with(*dirs::CONFIG) => None,
                dir if dir.starts_with(*dirs::SYSTEM) => None,
                dir if dir == *dirs::HOME => None,
                dir if !filename.starts_with('.') && dir.ends_with(".mise") => dir.parent(),
                dir if !filename.starts_with('.') && dir.ends_with(".config/mise") => {
                    dir.parent().unwrap().parent()
                }
                dir => Some(dir),
            },
            None => None,
        }
    }

    fn plugins(&self) -> eyre::Result<HashMap<String, String>> {
        self.plugins
            .clone()
            .into_iter()
            .map(|(k, v)| {
                let v = self.parse_template(&v)?;
                Ok((k, v))
            })
            .collect()
    }

    fn env_entries(&self) -> eyre::Result<Vec<EnvDirective>> {
        let env_entries = self.env.0.iter().cloned();
        let path_entries = self
            .env_path
            .iter()
            .map(|p| EnvDirective::Path(p.clone()))
            .collect_vec();
        let env_files = self
            .env_file
            .iter()
            .map(|p| EnvDirective::File(p.clone()))
            .collect_vec();
        let all = path_entries
            .into_iter()
            .chain(env_files)
            .chain(env_entries)
            .collect::<Vec<_>>();
        if !all.is_empty() {
            trust_check(&self.path)?;
        }
        Ok(all)
    }

    fn tasks(&self) -> Vec<&Task> {
        self.tasks.0.values().collect()
    }

    fn remove_plugin(&mut self, fa: &BackendArg) -> eyre::Result<()> {
        self.tools.shift_remove(fa);
        let doc = self.doc_mut()?;
        if let Some(tools) = doc.get_mut("tools") {
            if let Some(tools) = tools.as_table_like_mut() {
                tools.remove(&fa.to_string());
                if tools.is_empty() {
                    doc.as_table_mut().remove("tools");
                }
            }
        }
        Ok(())
    }

    fn replace_versions(
        &mut self,
        fa: &BackendArg,
        versions: &[(String, ToolVersionOptions)],
    ) -> eyre::Result<()> {
        let existing = self.tools.entry(fa.clone()).or_default();
        let output_empty_opts = |opts: &ToolVersionOptions| {
            if let Some(reg_ba) = REGISTRY_BACKEND_MAP
                .get(fa.short.as_str())
                .and_then(|b| b.first())
            {
                if reg_ba.opts.as_ref().is_some_and(|o| o == opts) {
                    // in this case the options specified are the same as in the registry so output no options and rely on the defaults
                    return true;
                }
            }
            opts.is_empty()
        };
        existing.0 = versions
            .iter()
            .map(|(v, opts)| MiseTomlTool {
                tt: ToolVersionType::Version(v.clone()),
                options: if !output_empty_opts(opts) {
                    Some(opts.clone())
                } else {
                    None
                },
            })
            .collect();
        let tools = self
            .doc_mut()?
            .entry("tools")
            .or_insert_with(table)
            .as_table_mut()
            .unwrap();

        // create a key from the short name preserving any decorations like prefix/suffix if the key already exists
        let key = get_key_with_decor(tools, fa.short.as_str());

        // if a short name is used like "node", make sure we remove any long names like "core:node"
        if fa.short != fa.full {
            tools.remove(&fa.full.to_string());
        }

        if versions.len() == 1 {
            if output_empty_opts(&versions[0].1) {
                tools.insert_formatted(&key, value(versions[0].0.clone()));
            } else {
                let mut table = InlineTable::new();
                table.insert("version", versions[0].0.to_string().into());
                for (k, v) in &versions[0].1 {
                    table.insert(k, v.clone().into());
                }
                tools.insert_formatted(&key, table.into());
            }
        } else {
            let mut arr = Array::new();
            for (v, opts) in versions {
                if output_empty_opts(opts) {
                    arr.push(v.to_string());
                } else {
                    let mut table = InlineTable::new();
                    table.insert("version", v.to_string().into());
                    for (k, v) in opts {
                        table.insert(k, v.clone().into());
                    }
                    arr.push(table);
                }
            }
            tools.insert_formatted(&key, Item::Value(Value::Array(arr)));
        }

        Ok(())
    }

    fn save(&self) -> eyre::Result<()> {
        let contents = self.dump()?;
        if let Some(parent) = self.path.parent() {
            create_dir_all(parent)?;
        }
        file::write(&self.path, contents)
    }

    fn dump(&self) -> eyre::Result<String> {
        Ok(self.doc()?.to_string())
    }

    fn source(&self) -> ToolSource {
        ToolSource::MiseToml(self.path.clone())
    }

    fn to_tool_request_set(&self) -> eyre::Result<ToolRequestSet> {
        let source = ToolSource::MiseToml(self.path.clone());
        let mut trs = ToolRequestSet::new();
        for (fa, tvp) in &self.tools {
            for tool in &tvp.0 {
                if let ToolVersionType::Path(_) = &tool.tt {
                    trust_check(&self.path)?;
                }
                let version = self.parse_template(&tool.tt.to_string())?;
                if let Some(mut options) = tool.options.clone() {
                    for v in options.values_mut() {
                        *v = self.parse_template(v)?;
                    }
                    let tvr = ToolRequest::new_opts(fa.clone(), &version, options, source.clone())?;
                    trs.add_version(tvr, &source);
                } else {
                    let tvr = ToolRequest::new(fa.clone(), &version, source.clone())?;
                    trs.add_version(tvr, &source);
                }
            }
        }
        Ok(trs)
    }

    fn aliases(&self) -> eyre::Result<AliasMap> {
        self.alias
            .clone()
            .iter()
            .map(|(k, v)| {
                let versions = v
                    .clone()
                    .versions
                    .into_iter()
                    .map(|(k, v)| {
                        let v = self.parse_template(&v)?;
                        Ok::<(String, String), eyre::Report>((k, v))
                    })
                    .collect::<eyre::Result<IndexMap<_, _>>>()?;
                Ok((
                    k.clone(),
                    Alias {
                        full: v.full.clone(),
                        versions,
                    },
                ))
            })
            .collect()
    }

    fn task_config(&self) -> &TaskConfig {
        &self.task_config
    }

    fn clone_box(&self) -> Box<dyn ConfigFile> {
        Box::new(self.clone())
    }
}

/// Returns a [`toml_edit::Key`] from the given `key`.
/// Preserves any surrounding whitespace (e.g. comments) if the key already exists in the provided [`toml_edit::Table`].
fn get_key_with_decor(table: &toml_edit::Table, key: &str) -> Key {
    let mut key = Key::from(key);
    if let Some((k, _)) = table.get_key_value(&key) {
        if let Some(prefix) = k.leaf_decor().prefix() {
            key.leaf_decor_mut().set_prefix(prefix.clone());
        }
        if let Some(suffix) = k.leaf_decor().suffix() {
            key.leaf_decor_mut().set_suffix(suffix.clone());
        }
    }
    key
}

impl Debug for MiseToml {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let tools = self.to_tool_request_set().unwrap().to_string();
        let title = format!("MiseToml({}): {tools}", &display_path(&self.path));
        let mut d = f.debug_struct(&title);
        if let Some(min_version) = &self.min_version {
            d.field("min_version", &min_version.to_string());
        }
        if !self.env_file.is_empty() {
            d.field("env_file", &self.env_file);
        }
        if let Ok(env) = self.env_entries() {
            if !env.is_empty() {
                d.field("env", &env);
            }
        }
        if !self.alias.is_empty() {
            d.field("alias", &self.alias);
        }
        if !self.plugins.is_empty() {
            d.field("plugins", &self.plugins);
        }
        if self.task_config.includes.is_some() {
            d.field("task_config", &self.task_config);
        }
        d.finish()
    }
}

impl Clone for MiseToml {
    fn clone(&self) -> Self {
        Self {
            min_version: self.min_version.clone(),
            context: self.context.clone(),
            path: self.path.clone(),
            env_file: self.env_file.clone(),
            env: self.env.clone(),
            env_path: self.env_path.clone(),
            alias: self.alias.clone(),
            doc: self.doc.clone(),
            tools: self.tools.clone(),
            plugins: self.plugins.clone(),
            tasks: self.tasks.clone(),
            task_config: self.task_config.clone(),
            settings: self.settings.clone(),
        }
    }
}

fn deserialize_version<'de, D>(deserializer: D) -> Result<Option<Versioning>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = serde::Deserialize::deserialize(deserializer)?;

    match s {
        Some(s) => Ok(Some(
            Versioning::new(&s)
                .ok_or(versions::Error::IllegalVersioning(s))
                .map_err(serde::de::Error::custom)?,
        )),
        None => Ok(None),
    }
}

impl<'de> de::Deserialize<'de> for EnvList {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct EnvManVisitor;

        impl<'de> Visitor<'de> for EnvManVisitor {
            type Value = EnvList;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("env table or array of env tables")
            }

            fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Self::Value, S::Error>
            where
                S: de::SeqAccess<'de>,
            {
                let mut env = vec![];
                while let Some(list) = seq.next_element::<EnvList>()? {
                    env.extend(list.0);
                }
                Ok(EnvList(env))
            }
            fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut env = vec![];
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "_" | "mise" => {
                            struct EnvDirectivePythonVenv {
                                path: PathBuf,
                                create: bool,
                            }

                            #[derive(Deserialize, Default)]
                            #[serde(deny_unknown_fields)]
                            struct EnvDirectivePython {
                                #[serde(default)]
                                venv: Option<EnvDirectivePythonVenv>,
                            }

                            #[derive(Deserialize)]
                            struct EnvDirectives {
                                #[serde(default, deserialize_with = "deserialize_path_entry_arr")]
                                path: Vec<PathEntry>,
                                #[serde(default, deserialize_with = "deserialize_arr")]
                                file: Vec<PathBuf>,
                                #[serde(default, deserialize_with = "deserialize_arr")]
                                source: Vec<PathBuf>,
                                #[serde(default)]
                                python: EnvDirectivePython,
                                #[serde(flatten)]
                                other: BTreeMap<String, toml::Value>,
                            }

                            impl<'de> de::Deserialize<'de> for EnvDirectivePythonVenv {
                                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                                where
                                    D: Deserializer<'de>,
                                {
                                    struct EnvDirectivePythonVenvVisitor;

                                    impl<'de> Visitor<'de> for EnvDirectivePythonVenvVisitor {
                                        type Value = EnvDirectivePythonVenv;
                                        fn expecting(
                                            &self,
                                            formatter: &mut Formatter,
                                        ) -> std::fmt::Result
                                        {
                                            formatter.write_str("python venv directive")
                                        }

                                        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                                        where
                                            E: de::Error,
                                        {
                                            Ok(EnvDirectivePythonVenv {
                                                path: v.into(),
                                                create: false,
                                            })
                                        }

                                        fn visit_map<M>(
                                            self,
                                            mut map: M,
                                        ) -> Result<Self::Value, M::Error>
                                        where
                                            M: de::MapAccess<'de>,
                                        {
                                            let mut path = None;
                                            let mut create = false;
                                            while let Some(key) = map.next_key::<String>()? {
                                                match key.as_str() {
                                                    "path" => {
                                                        path = Some(map.next_value()?);
                                                    }
                                                    "create" => {
                                                        create = map.next_value()?;
                                                    }
                                                    _ => {
                                                        return Err(de::Error::unknown_field(
                                                            &key,
                                                            &["path", "create"],
                                                        ));
                                                    }
                                                }
                                            }
                                            let path = path
                                                .ok_or_else(|| de::Error::missing_field("path"))?;
                                            Ok(EnvDirectivePythonVenv { path, create })
                                        }
                                    }

                                    const FIELDS: &[&str] = &["path", "create"];
                                    deserializer.deserialize_struct(
                                        "PythonVenv",
                                        FIELDS,
                                        EnvDirectivePythonVenvVisitor,
                                    )
                                }
                            }

                            let directives = map.next_value::<EnvDirectives>()?;
                            // TODO: parse these in the order they're defined somehow
                            for path in directives.path {
                                env.push(EnvDirective::Path(path));
                            }
                            for file in directives.file {
                                env.push(EnvDirective::File(file));
                            }
                            for source in directives.source {
                                env.push(EnvDirective::Source(source));
                            }
                            for (key, value) in directives.other {
                                env.push(EnvDirective::Module(key, value));
                            }
                            if let Some(venv) = directives.python.venv {
                                env.push(EnvDirective::PythonVenv {
                                    path: venv.path,
                                    create: venv.create,
                                });
                            }
                        }
                        _ => {
                            enum Val {
                                Int(i64),
                                Str(String),
                                Bool(bool),
                            }

                            impl<'de> de::Deserialize<'de> for Val {
                                fn deserialize<D>(
                                    deserializer: D,
                                ) -> std::result::Result<Self, D::Error>
                                where
                                    D: de::Deserializer<'de>,
                                {
                                    struct ValVisitor;

                                    impl Visitor<'_> for ValVisitor {
                                        type Value = Val;
                                        fn expecting(
                                            &self,
                                            formatter: &mut Formatter,
                                        ) -> std::fmt::Result
                                        {
                                            formatter.write_str("env value")
                                        }

                                        fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
                                        where
                                            E: de::Error,
                                        {
                                            match v {
                                                true => Err(de::Error::custom(
                                                    "env values cannot be true",
                                                )),
                                                false => Ok(Val::Bool(v)),
                                            }
                                        }

                                        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
                                        where
                                            E: de::Error,
                                        {
                                            Ok(Val::Int(v))
                                        }

                                        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                                        where
                                            E: de::Error,
                                        {
                                            Ok(Val::Str(v.to_string()))
                                        }
                                    }

                                    deserializer.deserialize_any(ValVisitor)
                                }
                            }

                            let value = map.next_value::<Val>()?;
                            match value {
                                Val::Int(i) => {
                                    env.push(EnvDirective::Val(key, i.to_string()));
                                }
                                Val::Str(s) => {
                                    env.push(EnvDirective::Val(key, s));
                                }
                                Val::Bool(_b) => env.push(EnvDirective::Rm(key)),
                            }
                        }
                    }
                }
                Ok(EnvList(env))
            }
        }

        deserializer.deserialize_any(EnvManVisitor)
    }
}

impl<'de> de::Deserialize<'de> for MiseTomlToolList {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct MiseTomlToolListVisitor;

        impl<'de> Visitor<'de> for MiseTomlToolListVisitor {
            type Value = MiseTomlToolList;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("tool list")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let tt: ToolVersionType = v
                    .parse()
                    .map_err(|e| de::Error::custom(format!("invalid tool: {e}")))?;
                Ok(MiseTomlToolList(vec![MiseTomlTool { tt, options: None }]))
            }

            fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Self::Value, S::Error>
            where
                S: de::SeqAccess<'de>,
            {
                let mut tools = vec![];
                while let Some(tool) = seq.next_element::<MiseTomlTool>()? {
                    tools.push(tool);
                }
                Ok(MiseTomlToolList(tools))
            }

            fn visit_map<M>(self, map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut options: BTreeMap<String, String> =
                    de::Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))?;
                let tt: ToolVersionType = options
                    .remove("version")
                    .or_else(|| options.remove("path").map(|p| format!("path:{p}")))
                    .or_else(|| options.remove("prefix").map(|p| format!("prefix:{p}")))
                    .or_else(|| options.remove("ref").map(|p| format!("ref:{p}")))
                    .ok_or_else(|| de::Error::custom("missing version"))?
                    .parse()
                    .map_err(de::Error::custom)?;
                Ok(MiseTomlToolList(vec![MiseTomlTool {
                    tt,
                    options: Some(options),
                }]))
            }
        }

        deserializer.deserialize_any(MiseTomlToolListVisitor)
    }
}

impl<'de> de::Deserialize<'de> for MiseTomlTool {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct MiseTomlToolVisitor;

        impl<'de> Visitor<'de> for MiseTomlToolVisitor {
            type Value = MiseTomlTool;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("tool definition")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let tt: ToolVersionType = v
                    .parse()
                    .map_err(|e| de::Error::custom(format!("invalid tool: {e}")))?;
                Ok(MiseTomlTool { tt, options: None })
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut options: BTreeMap<String, String> =
                    de::Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))?;
                let tt: ToolVersionType = options
                    .remove("version")
                    .or_else(|| options.remove("path").map(|p| format!("path:{p}")))
                    .or_else(|| options.remove("prefix").map(|p| format!("prefix:{p}")))
                    .or_else(|| options.remove("ref").map(|p| format!("ref:{p}")))
                    .ok_or_else(|| de::Error::custom("missing version"))?
                    .parse()
                    .map_err(de::Error::custom)?;
                Ok(MiseTomlTool {
                    tt,
                    options: Some(options),
                })
            }
        }

        deserializer.deserialize_any(MiseTomlToolVisitor)
    }
}

impl<'de> de::Deserialize<'de> for Tasks {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct TasksVisitor;

        impl<'de> Visitor<'de> for TasksVisitor {
            type Value = Tasks;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("task, string, or array of strings")
            }

            fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                struct TaskDef(Task);
                impl<'de> de::Deserialize<'de> for TaskDef {
                    fn deserialize<D>(deserializer: D) -> std::result::Result<TaskDef, D::Error>
                    where
                        D: de::Deserializer<'de>,
                    {
                        struct TaskDefVisitor;
                        impl<'de> Visitor<'de> for TaskDefVisitor {
                            type Value = TaskDef;
                            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                                formatter.write_str("task definition")
                            }

                            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                            where
                                E: de::Error,
                            {
                                Ok(TaskDef(Task {
                                    run: vec![v.to_string()],
                                    ..Default::default()
                                }))
                            }

                            fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
                            where
                                S: de::SeqAccess<'de>,
                            {
                                let mut run = vec![];
                                while let Some(s) = seq.next_element::<String>()? {
                                    run.push(s);
                                }
                                Ok(TaskDef(Task {
                                    run,
                                    ..Default::default()
                                }))
                            }

                            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
                            where
                                M: de::MapAccess<'de>,
                            {
                                let t = de::Deserialize::deserialize(
                                    de::value::MapAccessDeserializer::new(map),
                                )?;
                                Ok(TaskDef(t))
                            }
                        }
                        deserializer.deserialize_any(TaskDefVisitor)
                    }
                }
                let mut tasks = BTreeMap::new();
                while let Some(name) = map.next_key::<String>()? {
                    let mut task = map.next_value::<TaskDef>()?.0;
                    task.name.clone_from(&name);
                    tasks.insert(name, task);
                }
                Ok(Tasks(tasks))
            }
        }

        deserializer.deserialize_any(TasksVisitor)
    }
}

impl<'de> de::Deserialize<'de> for BackendArg {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct BackendArgVisitor;

        impl Visitor<'_> for BackendArgVisitor {
            type Value = BackendArg;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("backend argument")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(v.into())
            }
        }

        deserializer.deserialize_any(BackendArgVisitor)
    }
}

impl<'de> de::Deserialize<'de> for Alias {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct AliasVisitor;

        impl<'de> Visitor<'de> for AliasVisitor {
            type Value = Alias;
            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("alias")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Alias {
                    full: Some(v.to_string()),
                    ..Default::default()
                })
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let mut full = None;
                let mut versions = IndexMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "full" => {
                            full = Some(map.next_value()?);
                        }
                        "versions" => {
                            versions = map.next_value()?;
                        }
                        _ => {
                            deprecated!("TOOL_VERSION_ALIASES", "tool version aliases should be `alias.<TOOL>.versions.<FROM> = <TO>`, not `alias.<TOOL>.<FROM> = <TO>`");
                            versions.insert(key, map.next_value()?);
                        }
                    }
                }
                Ok(Alias { full, versions })
            }
        }

        deserializer.deserialize_any(AliasVisitor)
    }
}

#[cfg(test)]
mod tests {
    use indoc::formatdoc;
    use insta::{assert_debug_snapshot, assert_snapshot};
    use test_log::test;

    use crate::dirs::CWD;
    use crate::test::{replace_path, reset};

    use super::*;

    #[test]
    fn test_fixture() {
        reset();
        let cf = MiseToml::from_file(&dirs::HOME.join("fixtures/.mise.toml")).unwrap();

        assert_debug_snapshot!(cf.env_entries().unwrap());
        assert_debug_snapshot!(cf.plugins().unwrap());
        assert_snapshot!(replace_path(&format!(
            "{:#?}",
            cf.to_tool_request_set().unwrap()
        )));
        assert_debug_snapshot!(cf.alias);

        assert_snapshot!(replace_path(&format!("{:#?}", &cf)));
    }

    #[test]
    fn test_env() {
        reset();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
        min_version = "2024.1.1"
        [env]
        foo="bar"
        foo2='qux\nquux'
        foo3="qux\nquux"
        "#},
        )
        .unwrap();
        let cf = MiseToml::from_file(&p).unwrap();
        let dump = cf.dump().unwrap();
        let env = parse_env(file::read_to_string(&p).unwrap());

        assert_debug_snapshot!(env, @r#""foo=bar\nfoo2=qux\\nquux\nfoo3=qux\nquux""#);
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        with_settings!({
            assert_snapshot!(dump);
            assert_snapshot!(cf);
            assert_debug_snapshot!(cf);
        });
    }

    #[test]
    fn test_env_array_valid() {
        reset();
        let env = parse_env(formatdoc! {r#"
        [[env]]
        foo="bar"

        [[env]]
        bar="baz"

        [[env]]
        foo2='qux\nquux'
        bar2="qux\nquux"
        "#});

        assert_snapshot!(env, @r"
        foo=bar
        bar=baz
        foo2=qux\nquux
        bar2=qux
        quux
        ");
    }

    #[test]
    fn test_path_dirs() {
        reset();
        let env = parse_env(formatdoc! {r#"
            env_path=["/foo", "./bar"]
            [env]
            foo="bar"
            "#});

        assert_snapshot!(env, @r"
        path_add /foo
        path_add ./bar
        foo=bar
        ");

        let env = parse_env(formatdoc! {r#"
            env_path="./bar"
            "#});
        assert_snapshot!(env, @"path_add ./bar");

        let env = parse_env(formatdoc! {r#"
            [env]
            _.path = "./bar"
            "#});
        assert_debug_snapshot!(env, @r#""path_add ./bar""#);

        let env = parse_env(formatdoc! {r#"
            [env]
            _.path = ["/foo", "./bar"]
            "#});
        assert_snapshot!(env, @r"
        path_add /foo
        path_add ./bar
        ");

        let env = parse_env(formatdoc! {r#"
            [[env]]
            _.path = "/foo"
            [[env]]
            _.path = "./bar"
            "#});
        assert_snapshot!(env, @r"
        path_add /foo
        path_add ./bar
        ");

        let env = parse_env(formatdoc! {r#"
            env_path = "/foo"
            [env]
            _.path = "./bar"
            "#});
        assert_snapshot!(env, @r"
        path_add /foo
        path_add ./bar
        ");
    }

    #[test]
    fn test_env_file() {
        reset();
        let env = parse_env(formatdoc! {r#"
            env_file = ".env"
            "#});

        assert_debug_snapshot!(env, @r#""dotenv .env""#);

        let env = parse_env(formatdoc! {r#"
            env_file=[".env", ".env2"]
            "#});
        assert_debug_snapshot!(env, @r#""dotenv .env\ndotenv .env2""#);

        let env = parse_env(formatdoc! {r#"
            [env]
            _.file = ".env"
            "#});
        assert_debug_snapshot!(env, @r#""dotenv .env""#);

        let env = parse_env(formatdoc! {r#"
            [env]
            _.file = [".env", ".env2"]
            "#});
        assert_debug_snapshot!(env, @r#""dotenv .env\ndotenv .env2""#);

        let env = parse_env(formatdoc! {r#"
            dotenv = ".env"
            [env]
            _.file = ".env2"
            "#});
        assert_debug_snapshot!(env, @r#""dotenv .env\ndotenv .env2""#);
    }

    #[test]
    fn test_set_alias() {
        reset();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [alias.node.versions]
            16 = "16.0.0"
            18 = "18.0.0"
        "#},
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        let node = "node".into();
        let python = "python".into();
        cf.set_alias(&node, "18", "18.0.1").unwrap();
        cf.set_alias(&node, "20", "20.0.0").unwrap();
        cf.set_alias(&python, "3.10", "3.10.0").unwrap();

        assert_debug_snapshot!(cf.alias);
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf);
        file::remove_file(&p).unwrap();
    }

    #[test]
    fn test_remove_alias() {
        reset();
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [alias.node.versions]
            16 = "16.0.0"
            18 = "18.0.0"

            [alias.python.versions]
            "3.10" = "3.10.0"
            "#},
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        let node = "node".into();
        let python = "python".into();
        cf.remove_alias(&node, "16").unwrap();
        cf.remove_alias(&python, "3.10").unwrap();

        assert_debug_snapshot!(cf.alias);
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf.dump().unwrap());
        assert_snapshot!(cf);
        assert_debug_snapshot!(cf);
        file::remove_file(&p).unwrap();
    }

    #[test]
    fn test_replace_versions() {
        reset();
        let p = PathBuf::from("/tmp/.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            node = ["16.0.0", "18.0.0"]
            "#},
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        let node = "node".into();
        cf.replace_versions(
            &node,
            &[
                ("16.0.1".into(), Default::default()),
                ("18.0.1".into(), Default::default()),
            ],
        )
        .unwrap();

        assert_debug_snapshot!(cf.to_toolset().unwrap());
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf.dump().unwrap());
        assert_snapshot!(cf);
        assert_debug_snapshot!(cf);
        file::remove_all(&p).unwrap();
    }

    #[test]
    fn test_remove_plugin() {
        reset();
        let p = PathBuf::from("/tmp/.mise.toml");
        file::write(
            &p,
            formatdoc! {r#"
            [tools]
            node = ["16.0.0", "18.0.0"]
            "#},
        )
        .unwrap();
        let mut cf = MiseToml::from_file(&p).unwrap();
        cf.remove_plugin(&"node".into()).unwrap();

        assert_debug_snapshot!(cf.to_toolset().unwrap());
        let cf: Box<dyn ConfigFile> = Box::new(cf);
        assert_snapshot!(cf.dump().unwrap());
        assert_snapshot!(cf);
        assert_debug_snapshot!(cf);
    }

    #[test]
    fn test_env_entries() {
        reset();
        let toml = formatdoc! {r#"
        [env]
        foo1="1"
        rm=false
        _.path="/foo"
        foo2="2"
        _.file=".env"
        foo3="3"
        "#};
        assert_snapshot!(parse_env(toml), @r"
        foo1=1
        unset rm
        path_add /foo
        dotenv .env
        foo2=2
        foo3=3
        ");
    }

    #[test]
    fn test_env_arr() {
        reset();
        let toml = formatdoc! {r#"
        [[env]]
        foo1="1"
        rm=false
        _.path="/foo"
        foo2="2"
        _.file=".env"
        foo3="3"
        _.source="/baz1"

        [[env]]
        foo4="4"
        rm=false
        _.file=".env2"
        foo5="5"
        _.path="/bar"
        foo6="6"
        _.source="/baz2"
        "#};
        assert_snapshot!(parse_env(toml), @r"
        foo1=1
        unset rm
        path_add /foo
        dotenv .env
        source /baz1
        foo2=2
        foo3=3
        foo4=4
        unset rm
        path_add /bar
        dotenv .env2
        source /baz2
        foo5=5
        foo6=6
        ");
    }

    fn parse(s: String) -> MiseToml {
        let p = CWD.as_ref().unwrap().join(".test.mise.toml");
        file::write(&p, s).unwrap();
        let cfg = MiseToml::from_file(&p).unwrap();
        file::remove_file(&p).unwrap();

        cfg
    }

    fn parse_env(toml: String) -> String {
        parse(toml).env_entries().unwrap().into_iter().join("\n")
    }
}
