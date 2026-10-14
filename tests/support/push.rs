//! Owned HTTPS push provider with real VAPID verification and RFC 8291 decryption.

use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, Ordering},
    },
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;
use web_push_native::{
    Auth,
    jwt_simple::{
        algorithms::{ECDSAP256PublicKeyLike, ES256PublicKey},
        claims::NoCustomClaims,
        common::VerificationOptions,
    },
    p256::SecretKey,
};

#[derive(Clone)]
pub struct CapturedPush {
    pub payload: serde_json::Value,
    pub vapid_valid: bool,
}

pub struct TestPushProvider {
    pub address: SocketAddr,
    pub certificate: Vec<u8>,
    captured: Arc<Mutex<Vec<CapturedPush>>>,
    status: Arc<AtomicU16>,
    silent: Arc<AtomicBool>,
    retry_after: Arc<Mutex<Option<String>>>,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

impl TestPushProvider {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["push.example.test".into()]).unwrap();
        let certificate = cert.der().to_vec();
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
        let captured = Arc::new(Mutex::new(Vec::new()));
        let status = Arc::new(AtomicU16::new(201));
        let silent = Arc::new(AtomicBool::new(false));
        let retry_after = Arc::new(Mutex::new(None::<String>));
        let cancel = CancellationToken::new();
        let task = {
            let captured = captured.clone();
            let status = status.clone();
            let silent = silent.clone();
            let retry_after = retry_after.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move {
                let mut connections = JoinSet::new();
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        accepted = listener.accept() => {
                            let (socket,_) = accepted.unwrap();
                            let captured = captured.clone();
                            let status = status.clone();
                            let silent = silent.clone();
                            let retry_after = retry_after.clone();
                            let acceptor = acceptor.clone();
                            let cancel = cancel.clone();
                            connections.spawn(async move {
                                tokio::select! {
                                    _ = cancel.cancelled() => {},
                                    _ = async {
                                        let mut socket = acceptor.accept(socket).await?;
                                        let mut bytes = Vec::new();
                                        let boundary = loop {
                                            let mut chunk = [0;1024];
                                            let length = socket.read(&mut chunk).await?;
                                            if length==0 || bytes.len()>16384 { return Ok::<_,std::io::Error>(()); }
                                            bytes.extend_from_slice(&chunk[..length]);
                                            if let Some(position) = bytes.windows(4).position(|window| window==b"\r\n\r\n") { break position+4; }
                                        };
                                        let headers = String::from_utf8_lossy(&bytes[..boundary]).to_ascii_lowercase();
                                        let original_headers = String::from_utf8_lossy(&bytes[..boundary]).to_string();
                                        let header = |name:&str| original_headers.lines().find_map(|line| {
                                            let (field,value)=line.split_once(':')?;
                                            field.eq_ignore_ascii_case(name).then(||value.trim().to_owned())
                                        });
                                        let length:usize=header("content-length").unwrap().parse().unwrap();
                                        if length>16384 { return Ok(()); }
                                        while bytes.len()<boundary+length {
                                            let mut chunk=[0;1024];
                                            let read=socket.read(&mut chunk).await?;
                                            if read==0 { return Ok(()); }
                                            bytes.extend_from_slice(&chunk[..read]);
                                        }
                                        assert!(headers.contains("content-encoding: aes128gcm"));
                                        let authorization=header("authorization").unwrap();
                                        let (token,public)=authorization.strip_prefix("vapid t=").unwrap().split_once(", k=").unwrap();
                                        let public=ES256PublicKey::from_bytes(&URL_SAFE_NO_PAD.decode(public).unwrap()).unwrap();
                                        let options=VerificationOptions {
                                            allowed_audiences:Some(["https://push.example.test".to_owned()].into_iter().collect()),
                                            required_subject:Some("mailto:operator@example.test".into()),
                                            ..Default::default()
                                        };
                                        let vapid_valid=public.verify_token::<NoCustomClaims>(token,Some(options)).is_ok();
                                        let payload=web_push_native::decrypt(bytes[boundary..boundary+length].to_vec(),&SecretKey::from_slice(&[8;32]).unwrap(),&Auth::clone_from_slice(&[9;16])).unwrap();
                                        captured.lock().unwrap().push(CapturedPush {payload:serde_json::from_slice(&payload).unwrap(),vapid_valid});
                                        if silent.load(Ordering::Relaxed) { cancel.cancelled().await; return Ok(()); }
                                        let status=status.load(Ordering::Relaxed);
                                        let retry=retry_after.lock().unwrap().as_ref().map(|value| format!("Retry-After: {value}\r\n")).unwrap_or_default();
                                        let location=if status==302 {"Location: https://127.0.0.1/private\r\n"} else {""};
                                        socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Length: 0\r\nConnection: close\r\n{retry}{location}\r\n").as_bytes()).await?;
                                        Ok(())
                                    } => {},
                                }
                            });
                        },
                        _ = connections.join_next(),if !connections.is_empty() => {},
                    }
                }
            })
        };
        Self {
            address,
            certificate,
            captured,
            status,
            silent,
            retry_after,
            cancel,
            task,
        }
    }

    pub fn requests(&self) -> Vec<CapturedPush> {
        self.captured.lock().unwrap().clone()
    }

    pub fn respond(&self, status: u16, retry_after: Option<&str>) {
        self.status.store(status, Ordering::Relaxed);
        *self.retry_after.lock().unwrap() = retry_after.map(str::to_owned);
    }

    pub fn set_silent(&self) {
        self.silent.store(true, Ordering::Relaxed);
    }
}

impl Drop for TestPushProvider {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}
