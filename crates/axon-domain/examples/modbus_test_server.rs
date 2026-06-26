//! A fixed-port in-process Modbus TCP server for the `modbus_roundtrip.sh`
//! gate's demo leg: serves a 16-register holding bank + 16 coils on
//! 127.0.0.1:<PORT> (default 15502). Used ONLY for verification — it stands up,
//! the `.ax` demo connects to it through the shim, writes/reads, and the script
//! tears it down. NOT shipped in any product path.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_modbus::prelude::*;
use tokio_modbus::server::tcp::{accept_tcp_connection, Server};

#[derive(Clone)]
struct Svc {
    holdings: Arc<Mutex<Vec<u16>>>,
    coils: Arc<Mutex<Vec<bool>>>,
}

impl tokio_modbus::server::Service for Svc {
    type Request = Request<'static>;
    type Response = Response;
    type Exception = ExceptionCode;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Response, ExceptionCode>> + Send>,
    >;

    fn call(&self, req: Self::Request) -> Self::Future {
        let holdings = self.holdings.clone();
        let coils = self.coils.clone();
        Box::pin(async move {
            match req {
                Request::ReadHoldingRegisters(a, c) => {
                    let h = holdings.lock().await;
                    let (a, c) = (a as usize, c as usize);
                    if a + c > h.len() {
                        return Err(ExceptionCode::IllegalDataAddress);
                    }
                    Ok(Response::ReadHoldingRegisters(h[a..a + c].to_vec()))
                }
                Request::WriteSingleRegister(a, v) => {
                    let mut h = holdings.lock().await;
                    if a as usize >= h.len() {
                        return Err(ExceptionCode::IllegalDataAddress);
                    }
                    h[a as usize] = v;
                    Ok(Response::WriteSingleRegister(a, v))
                }
                Request::ReadCoils(a, c) => {
                    let coils = coils.lock().await;
                    let (a, n) = (a as usize, c as usize);
                    if a + n > coils.len() {
                        return Err(ExceptionCode::IllegalDataAddress);
                    }
                    Ok(Response::ReadCoils(coils[a..a + n].to_vec()))
                }
                Request::WriteSingleCoil(a, on) => {
                    let mut coils = coils.lock().await;
                    if a as usize >= coils.len() {
                        return Err(ExceptionCode::IllegalDataAddress);
                    }
                    coils[a as usize] = on;
                    Ok(Response::WriteSingleCoil(a, on))
                }
                _ => Err(ExceptionCode::IllegalFunction),
            }
        })
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let port: u16 = std::env::var("MODBUS_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(15502);
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    eprintln!("modbus_test_server: listening on {addr}");
    let svc = Svc {
        holdings: Arc::new(Mutex::new(vec![0u16; 16])),
        coils: Arc::new(Mutex::new(vec![false; 16])),
    };
    let server = Server::new(listener);
    let new_service = move |_sock| Ok(Some(svc.clone()));
    let on_connected = move |stream, sa| {
        let ns = new_service.clone();
        async move { accept_tcp_connection(stream, sa, ns) }
    };
    let on_err = |e| eprintln!("modbus_test_server err: {e}");
    let _ = server.serve(&on_connected, on_err).await;
}
