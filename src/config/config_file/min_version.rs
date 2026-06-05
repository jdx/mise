use versions::Versioning;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MinVersionSpec {
    hard: Option<Versioning>,
    soft: Option<Versioning>,
}

impl MinVersionSpec {
    pub fn new(hard: Option<Versioning>, soft: Option<Versioning>) -> Option<Self> {
        if hard.is_none() && soft.is_none() {
            None
        } else {
            Some(Self { hard, soft })
        }
    }

    fn hard(&self) -> Option<&Versioning> {
        self.hard.as_ref()
    }

    fn soft(&self) -> Option<&Versioning> {
        self.soft.as_ref()
    }

    pub fn hard_violation(&self, current: &Versioning) -> Option<&Versioning> {
        self.hard().filter(|required| current < *required)
    }

    pub fn soft_violation(&self, current: &Versioning) -> Option<&Versioning> {
        self.soft().filter(|recommended| current < *recommended)
    }
}
