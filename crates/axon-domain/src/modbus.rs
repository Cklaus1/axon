//! `native::modbus` — Modbus TCP client (industrial).
//!
//! A real, locally-verifiable Modbus TCP client backed by `tokio-modbus`. The
//! verification (see `scripts/modbus_roundtrip.sh` / the `#[test]` below) spins
//! up an IN-TEST `tokio-modbus` TCP **server**, connects from the shim, writes
//! a holding register, reads it back, and asserts equality — a true protocol
//! round-trip, not a stub.
//!
//! Surface (R13 representable set):
//!  * `modbus_connect(host: str, port: i64) -> Handle`   (Conn, affine resource)
//!  * `modbus_read_holding(ref h: Conn, addr: i64, count: i64) -> [i64]`
//!  * `modbus_write_register(ref h: Conn, addr: i64, val: i64) -> Unit`
//!  * `modbus_read_coils(ref h: Conn, addr: i64, count: i64) -> [i64]`
//!  * `modbus_write_coil(ref h: Conn, addr: i64, on: i64) -> Unit`
//!  * `modbus_close(h: Conn) -> Unit`   (consumes the connection)
//!
//! Codegen E0910-refuses these (live network I/O — the `host_await`/native
//! precedent). Net-host pinning is enforced at CHECK time against the
//! `modbus_connect` host literal via the existing net-cap allowlist.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio_modbus::prelude::*;

use crate::{DomainArg, DomainResult, DomainValue, Slab};

/// A live Modbus TCP connection. An affine resource handle; consumed by
/// `modbus_close`. Holds the `tokio-modbus` context + the runtime it runs on.
pub struct ModbusConn {
    ctx: client::Context,
    rt: Runtime,
}

impl std::fmt::Debug for ModbusConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ModbusConn(..)")
    }
}

#[derive(Debug, Default)]
pub struct ModbusBackend {
    conns: Slab<ModbusConn>,
}

impl ModbusBackend {
    pub fn dispatch(&mut self, fnname: &str, args: &[DomainArg]) -> DomainResult {
        match (fnname, args) {
            ("modbus_connect", [DomainArg::Str(host), DomainArg::Int(port)]) => {
                self.connect(host, *port)
            }
            (
                "modbus_read_holding",
                [DomainArg::Handle { payload, .. }, DomainArg::Int(addr), DomainArg::Int(count)],
            ) => self.read_holding(*payload, *addr, *count),
            (
                "modbus_write_register",
                [DomainArg::Handle { payload, .. }, DomainArg::Int(addr), DomainArg::Int(val)],
            ) => self.write_register(*payload, *addr, *val),
            (
                "modbus_read_coils",
                [DomainArg::Handle { payload, .. }, DomainArg::Int(addr), DomainArg::Int(count)],
            ) => self.read_coils(*payload, *addr, *count),
            (
                "modbus_write_coil",
                [DomainArg::Handle { payload, .. }, DomainArg::Int(addr), DomainArg::Int(on)],
            ) => self.write_coil(*payload, *addr, *on),
            ("modbus_close", [DomainArg::Handle { payload, .. }]) => {
                let mut conn = self.conns.free(*payload)?;
                conn.rt.block_on(async {
                    let _ = conn.ctx.disconnect().await;
                });
                Ok(DomainValue::Unit)
            }
            _ => Err(format!(
                "native::modbus: bad call `{fnname}` (wrong argument shape)"
            )),
        }
    }

    fn connect(&mut self, host: &str, port: i64) -> DomainResult {
        let port =
            u16::try_from(port).map_err(|_| "modbus_connect: port out of range".to_string())?;
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|e| format!("modbus_connect: bad address `{host}:{port}`: {e}"))?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("modbus_connect: runtime: {e}"))?;
        let ctx = rt
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(5), tcp::connect(addr)).await
            })
            .map_err(|_| format!("modbus_connect: timed out connecting to {addr}"))?
            .map_err(|e| format!("modbus_connect: {e}"))?;
        let idx = self.conns.insert(ModbusConn { ctx, rt });
        Ok(DomainValue::Handle {
            name: "Conn",
            payload: idx,
        })
    }

    fn read_holding(&mut self, h: i64, addr: i64, count: i64) -> DomainResult {
        let addr = reg_addr(addr)?;
        let count = reg_count(count)?;
        let conn = self.conns.get_mut(h)?;
        let regs = conn
            .rt
            .block_on(async { conn.ctx.read_holding_registers(addr, count).await })
            .map_err(|e| format!("modbus_read_holding: {e}"))?
            .map_err(|e| format!("modbus_read_holding: exception {e:?}"))?;
        Ok(DomainValue::IntArray(
            regs.into_iter().map(|r| r as i64).collect(),
        ))
    }

    fn write_register(&mut self, h: i64, addr: i64, val: i64) -> DomainResult {
        let addr = reg_addr(addr)?;
        let val = u16::try_from(val)
            .map_err(|_| "modbus_write_register: value out of u16 range".to_string())?;
        let conn = self.conns.get_mut(h)?;
        conn.rt
            .block_on(async { conn.ctx.write_single_register(addr, val).await })
            .map_err(|e| format!("modbus_write_register: {e}"))?
            .map_err(|e| format!("modbus_write_register: exception {e:?}"))?;
        Ok(DomainValue::Unit)
    }

    fn read_coils(&mut self, h: i64, addr: i64, count: i64) -> DomainResult {
        let addr = reg_addr(addr)?;
        let count = reg_count(count)?;
        let conn = self.conns.get_mut(h)?;
        let coils = conn
            .rt
            .block_on(async { conn.ctx.read_coils(addr, count).await })
            .map_err(|e| format!("modbus_read_coils: {e}"))?
            .map_err(|e| format!("modbus_read_coils: exception {e:?}"))?;
        Ok(DomainValue::IntArray(
            coils.into_iter().map(|b| b as i64).collect(),
        ))
    }

    fn write_coil(&mut self, h: i64, addr: i64, on: i64) -> DomainResult {
        let addr = reg_addr(addr)?;
        let conn = self.conns.get_mut(h)?;
        conn.rt
            .block_on(async { conn.ctx.write_single_coil(addr, on != 0).await })
            .map_err(|e| format!("modbus_write_coil: {e}"))?
            .map_err(|e| format!("modbus_write_coil: exception {e:?}"))?;
        Ok(DomainValue::Unit)
    }
}

fn reg_addr(addr: i64) -> Result<u16, String> {
    u16::try_from(addr).map_err(|_| "modbus: register address out of u16 range".to_string())
}

fn reg_count(count: i64) -> Result<u16, String> {
    if count <= 0 {
        return Err("modbus: count must be positive".to_string());
    }
    u16::try_from(count).map_err(|_| "modbus: count out of u16 range".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio_modbus::server::tcp::{accept_tcp_connection, Server};

    // A minimal in-test Modbus TCP server backing a 16-register holding bank +
    // 16 coils, so the round-trip is a real protocol exchange.
    #[derive(Clone)]
    struct TestService {
        holdings: Arc<Mutex<Vec<u16>>>,
        coils: Arc<Mutex<Vec<bool>>>,
    }

    impl tokio_modbus::server::Service for TestService {
        type Request = Request<'static>;
        type Response = Response;
        type Exception = ExceptionCode;
        type Future = std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Self::Response, Self::Exception>> + Send>,
        >;

        fn call(&self, req: Self::Request) -> Self::Future {
            let holdings = self.holdings.clone();
            let coils = self.coils.clone();
            Box::pin(async move {
                match req {
                    Request::ReadHoldingRegisters(addr, cnt) => {
                        let h = holdings.lock().await;
                        let a = addr as usize;
                        let c = cnt as usize;
                        if a + c > h.len() {
                            return Err(ExceptionCode::IllegalDataAddress);
                        }
                        Ok(Response::ReadHoldingRegisters(h[a..a + c].to_vec()))
                    }
                    Request::WriteSingleRegister(addr, val) => {
                        let mut h = holdings.lock().await;
                        let a = addr as usize;
                        if a >= h.len() {
                            return Err(ExceptionCode::IllegalDataAddress);
                        }
                        h[a] = val;
                        Ok(Response::WriteSingleRegister(addr, val))
                    }
                    Request::ReadCoils(addr, cnt) => {
                        let c = coils.lock().await;
                        let a = addr as usize;
                        let n = cnt as usize;
                        if a + n > c.len() {
                            return Err(ExceptionCode::IllegalDataAddress);
                        }
                        Ok(Response::ReadCoils(c[a..a + n].to_vec()))
                    }
                    Request::WriteSingleCoil(addr, on) => {
                        let mut c = coils.lock().await;
                        let a = addr as usize;
                        if a >= c.len() {
                            return Err(ExceptionCode::IllegalDataAddress);
                        }
                        c[a] = on;
                        Ok(Response::WriteSingleCoil(addr, on))
                    }
                    _ => Err(ExceptionCode::IllegalFunction),
                }
            })
        }
    }

    fn spawn_server(rt: &Runtime) -> SocketAddr {
        let listener = rt
            .block_on(async { tokio::net::TcpListener::bind("127.0.0.1:0").await })
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let service = TestService {
            holdings: Arc::new(Mutex::new(vec![0u16; 16])),
            coils: Arc::new(Mutex::new(vec![false; 16])),
        };
        rt.spawn(async move {
            let server = Server::new(listener);
            let new_service = move |_socket| Ok(Some(service.clone()));
            let on_connected = move |stream, socket_addr| {
                let ns = new_service.clone();
                async move { accept_tcp_connection(stream, socket_addr, ns) }
            };
            let on_err = |err| eprintln!("test modbus server err: {err}");
            let _ = server.serve(&on_connected, on_err).await;
        });
        // Give the listener a moment to be ready.
        std::thread::sleep(Duration::from_millis(100));
        addr
    }

    #[test]
    fn write_then_read_holding_register_roundtrip() {
        // A dedicated runtime hosts the test server (multi-thread so it keeps
        // serving while the blocking client drives its own current-thread rt).
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let addr = spawn_server(&server_rt);

        let mut b = ModbusBackend::default();
        let h = match b
            .dispatch(
                "modbus_connect",
                &[
                    DomainArg::Str(addr.ip().to_string()),
                    DomainArg::Int(addr.port() as i64),
                ],
            )
            .unwrap()
        {
            DomainValue::Handle { payload, .. } => payload,
            _ => panic!("handle"),
        };
        let hh = DomainArg::Handle {
            tag: crate::tag_for("Conn"),
            payload: h,
        };
        // Write 0x1234 to holding register 3.
        b.dispatch(
            "modbus_write_register",
            &[hh.clone(), DomainArg::Int(3), DomainArg::Int(0x1234)],
        )
        .unwrap();
        // Read it back.
        let read = b
            .dispatch(
                "modbus_read_holding",
                &[hh.clone(), DomainArg::Int(3), DomainArg::Int(1)],
            )
            .unwrap();
        assert_eq!(read, DomainValue::IntArray(vec![0x1234]));

        // Coil round-trip.
        b.dispatch(
            "modbus_write_coil",
            &[hh.clone(), DomainArg::Int(5), DomainArg::Int(1)],
        )
        .unwrap();
        let coils = b
            .dispatch(
                "modbus_read_coils",
                &[hh.clone(), DomainArg::Int(5), DomainArg::Int(1)],
            )
            .unwrap();
        assert_eq!(coils, DomainValue::IntArray(vec![1]));

        b.dispatch("modbus_close", &[hh]).unwrap();
    }

    #[test]
    fn bad_handle_is_graceful_err() {
        let mut b = ModbusBackend::default();
        for bad in [9999i64, -1, i64::MIN, i64::MAX] {
            let h = DomainArg::Handle {
                tag: crate::tag_for("Conn"),
                payload: bad,
            };
            assert!(b
                .dispatch(
                    "modbus_read_holding",
                    &[h, DomainArg::Int(0), DomainArg::Int(1)]
                )
                .is_err());
        }
    }
}
