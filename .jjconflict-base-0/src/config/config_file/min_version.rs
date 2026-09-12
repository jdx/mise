use versions::Versioning;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MinVersionSpec {
    hard: Option<Versioning>,
    soft: Option<Versioning>,
}

impl MinVersionSpec {
    pub(crate) fn new(hard: Option<Versioning>, soft: Option<Versioning>) -> Option<Self> {
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

    pub(crate) fn hard_violation(&self, current: &Versioning) -> Option<&Versioning> {
        self.hard().filter(|required| current < *required)
    }

    pub(crate) fn soft_violation(&self, current: &Versioning) -> Option<&Versioning> {
        self.soft().filter(|recommended| current < *recommended)
    }
}
