use indexmap::IndexSet;

#[derive(Default, Clone, Debug, serde::Deserialize)]
pub struct Redactions {
    #[serde(default)]
    pub env: IndexSet<String>,
    #[serde(default)]
    pub vars: IndexSet<String>,
}

impl Redactions {
    pub fn merge(&mut self, other: Self) {
        for e in other.env {
            self.env.insert(e);
        }
        for v in other.vars {
            self.vars.insert(v);
        }
    }

    pub fn render(&mut self, tera: &mut tera::Tera, ctx: &tera::Context) -> eyre::Result<()> {
        for r in self.env.clone().drain(..) {
            self.env.insert(tera.render_str(&r, ctx)?);
        }
        for r in self.vars.clone().drain(..) {
            self.vars.insert(tera.render_str(&r, ctx)?);
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.env.is_empty() && self.vars.is_empty()
    }
}
