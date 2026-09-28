use super::*;

#[tokio::test]
async fn test_http_operation_is_cancelled_on_interrupt() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/pending", listener.local_addr().unwrap());
    let cancellation = HttpCancellation::default();
    let trigger = cancellation.clone();
    let (release_tx, release_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 1024];
        let bytes_read = stream.read(&mut request).unwrap();
        assert!(bytes_read > 0, "client closed before sending its request");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n")
            .unwrap();
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(50));
        trigger.cancel();
        release_rx.recv().unwrap();
    });

    let lua = Lua::new();
    let input = lua.create_table().unwrap();
    input.set("url", url).unwrap();
    let err = get_with_cancellation(&lua, input, &cancellation)
        .await
        .unwrap_err();

    assert_eq!(err.to_string(), "runtime error: interrupted");
    let mut later = cancellation.subscribe();
    assert!(
        tokio::time::timeout(Duration::from_millis(10), later.cancelled())
            .await
            .is_err(),
        "a later operation should wait for the next cancellation generation"
    );
    release_tx.send(()).unwrap();
    server.join().unwrap();
}
