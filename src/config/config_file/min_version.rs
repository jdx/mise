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

    pub fn soft(&self) -> Option<&Versioning> {
        self.soft.as_ref()
    }

    pub fn hard_violation(&self, current: &Versioning) -> Option<&Versioning> {
        self.hard().filter(|required| current < *required)
    }

    pub fn soft_violation(&self, current: &Versioning) -> Option<&Versioning> {
        self.soft().filter(|recommended| current < *recommended)
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
