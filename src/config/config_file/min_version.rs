use std::fmt;
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

    pub fn hard_violation(&self, current: &Versioning) -> Option<&Versioning> {
        self.hard().filter(|required| current < *required)
    }

    pub fn soft_violation(&self, current: &Versioning) -> Option<&Versioning> {
        self.soft().filter(|recommended| current < *recommended)
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

impl fmt::Display for MinVersionSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.hard.as_ref(), self.soft.as_ref()) {
            (Some(h), None) => write!(f, "{}", h),
            (None, Some(s)) => write!(f, "{}", s),
            (Some(h), Some(s)) => write!(f, "hard={}, soft={}", h, s),
            (None, None) => write!(f, ""),
        }
    }
}
