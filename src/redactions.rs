use indexmap::IndexSet;

#[derive(Default, Clone, Debug, serde::Deserialize)]
pub struct Redactions(pub IndexSet<String>);

impl Redactions {
    pub fn merge(&mut self, other: Self) {
        self.0.extend(other.0);
    }

    pub fn render(&mut self, tera: &mut tera::Tera, ctx: &tera::Context) -> eyre::Result<()> {
        for r in self.0.clone().drain(..) {
            self.0.insert(tera.render_str(&r, ctx)?);
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
