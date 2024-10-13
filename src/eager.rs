use xx::regex;

/// initializes slow parts of mise eagerly in the background
pub fn early_init() {
    rayon::spawn(|| {
        regex!("");
    }); // initialize regex library
}

/// run after SETTING has been loaded
pub fn post_settings() {
    // if std::env::var("EAGER").is_err() {
    //   return;
    // }
    // rayon::spawn(|s| {
    //     s.spawn(|_| {
    //         // let _ = load_toolset();
    //     });
    // });
}
