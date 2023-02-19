use crate::ui::progress_report::ProgressReport;
use indicatif::MultiProgress;

pub struct MultiProgressReport {
    mp: Option<MultiProgress>,
}

impl MultiProgressReport {
    pub fn new(verbose: bool) -> Self {
        match verbose {
            true => Self { mp: None },
            false => Self {
                mp: Some(MultiProgress::new()),
            },
        }
    }
    pub fn add(&self) -> ProgressReport {
        match &self.mp {
            Some(mp) => {
                let mut pr = ProgressReport::new(false);
                pr.pb = Some(mp.add(pr.pb.unwrap()));
                pr
            }
            None => ProgressReport::new(true),
        }
    }
    // pub fn clear(&self) {
    //     match &self.mp {
    //         Some(mp) => {
    //             let _ = mp.clear();
    //         },
    //         None => ()
    //     }
    // }
}
