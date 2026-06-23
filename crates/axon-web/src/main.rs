fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port = flag_value(&args, "--port").unwrap_or_else(|| "8080".into());
    let axon_bin = std::env::var("AXON_BIN")
        .ok()
        .or_else(|| flag_value(&args, "--axon"))
        .unwrap_or_else(|| "axon".into());

    let addr = format!("127.0.0.1:{port}");
    let srv = tiny_http::Server::http(&addr)
        .unwrap_or_else(|e| panic!("axon-web: cannot bind {addr}: {e}"));
    eprintln!("axon-web listening on http://{addr}  (axon={axon_bin})");

    for req in srv.incoming_requests() {
        axon_web::server::handle(req, &axon_bin);
    }
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}
