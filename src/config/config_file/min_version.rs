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
            Some(Self {
                hard,
                soft,
                upgrade_instructions: None,
            })
        }
    }

    pub fn hard(&self) -> Option<&Versioning> {
        self.hard.as_ref()
    }

    pub fn set_hard(&mut self, version: Versioning) {
        self.hard = Some(version);
    }

    pub fn soft(&self) -> Option<&Versioning> {
        self.soft.as_ref()
    }

    pub fn set_soft(&mut self, version: Versioning) {
        self.soft = Some(version);
    }

    pub fn is_empty(&self) -> bool {
        self.hard.is_none() && self.soft.is_none()
    }

    pub fn hard_violation<'a>(&'a self, current: &Versioning) -> Option<&'a Versioning> {
        self.hard().filter(|required| current < *required)
    }

    pub fn soft_violation<'a>(&'a self, current: &Versioning) -> Option<&'a Versioning> {
        self.soft().filter(|recommended| current < *recommended)
    }

    pub fn to_owned(&self) -> Self {
        Self {
            hard: self.hard.clone(),
            soft: self.soft.clone(),
            upgrade_instructions: self.upgrade_instructions.clone(),
        }
    }

    pub fn merge_with(&mut self, other: &Self) {
        if self.hard.is_none() {
            self.hard = other.hard.clone();
        }
        if self.soft.is_none() {
            self.soft = other.soft.clone();
        }
    }
}
