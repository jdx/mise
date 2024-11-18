use once_cell::sync::Lazy;

pub static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_io()
        .enable_time()
        .build()
        .unwrap()
});
