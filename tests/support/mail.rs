//! Owned SMTP/TLS capture with ephemeral keys; no mail is forwarded externally.

use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use mailparse::MailHeaderMap;
use tokio::sync::Notify;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, PartialEq)]
pub enum SmtpEncryption {
    Plaintext,
    StartTls,
    Implicit,
}

#[derive(Clone, Copy)]
pub enum SmtpBehavior {
    Accept,
    TemporaryFailure,
    PermanentFailure,
    Silent,
    PauseAfterData,
}

#[derive(Clone)]
pub struct CapturedMail {
    pub recipient: String,
    pub subject: String,
    pub text: String,
}

pub struct TestSmtpServer {
    pub address: SocketAddr,
    pub certificate: Vec<u8>,
    commands: Arc<Mutex<Vec<String>>>,
    messages: Arc<Mutex<Vec<CapturedMail>>>,
    behavior: Arc<Mutex<SmtpBehavior>>,
    encryption: SmtpEncryption,
    accepted_connections: Arc<AtomicUsize>,
    acknowledge_data: Arc<Notify>,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

impl TestSmtpServer {
    pub async fn start() -> Self {
        Self::start_with(SmtpEncryption::Plaintext, SmtpBehavior::Accept).await
    }

    pub async fn start_with(encryption: SmtpEncryption, behavior: SmtpBehavior) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".into(), "localhost".into()])
                .unwrap();
        let certificate = cert.der().to_vec();
        // Explicit provider selection avoids the application's combined ring/aws-lc features.
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
        let acceptor = TlsAcceptor::from(Arc::new(tls));
        let commands = Arc::new(Mutex::new(Vec::new()));
        let messages = Arc::new(Mutex::new(Vec::new()));
        let behavior = Arc::new(Mutex::new(behavior));
        let cancel = CancellationToken::new();
        let accepted_connections = Arc::new(AtomicUsize::new(0));
        let acknowledge_data = Arc::new(Notify::new());
        let task = {
            let commands = commands.clone();
            let messages = messages.clone();
            let behavior = behavior.clone();
            let cancel = cancel.clone();
            let accepted_connections = accepted_connections.clone();
            let acknowledge_data = acknowledge_data.clone();
            tokio::spawn(async move {
                let mut connections = JoinSet::new();
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        accepted = listener.accept() => {
                            let (stream, _) = accepted.unwrap();
                            accepted_connections.fetch_add(1, Ordering::Relaxed);
                            let commands = commands.clone();
                            let messages = messages.clone();
                            let behavior = behavior.clone();
                            let cancel = cancel.clone();
                            let acceptor = acceptor.clone();
                            let acknowledge_data = acknowledge_data.clone();
                            connections.spawn(async move {
                                tokio::select! {
                                    _ = cancel.cancelled() => {},
                                    // Disconnect/timeout/TLS rejection is ephemeral. JoinSet
                                    // owns every connection and aborts it during fixture teardown.
                                    _ = capture(stream, encryption, behavior, acceptor, commands, messages, acknowledge_data) => {},
                                }
                            });
                        },
                        _ = connections.join_next(), if !connections.is_empty() => {},
                    }
                }
            })
        };

        Self {
            address,
            certificate,
            commands,
            messages,
            behavior,
            encryption,
            accepted_connections,
            acknowledge_data,
            cancel,
            task,
        }
    }

    pub fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }

    pub fn messages(&self) -> Vec<CapturedMail> {
        self.messages.lock().unwrap().clone()
    }

    pub fn set_behavior(&self, behavior: SmtpBehavior) {
        *self.behavior.lock().unwrap() = behavior;
    }

    pub fn connections(&self) -> usize {
        self.accepted_connections.load(Ordering::Relaxed)
    }

    pub fn acknowledge(&self) {
        self.acknowledge_data.notify_one();
    }

    #[cfg(feature = "test-failpoints")]
    pub fn configuration(&self) -> chat_server::infrastructure::mail::MailConfig {
        use chat_server::infrastructure::mail::{FixtureMailTransport, MailConfig, SmtpTlsMode};

        MailConfig {
            public_origin: "https://chat.example.test".into(),
            host: self.address.ip().to_string(),
            port: self.address.port(),
            tls_mode: if self.encryption == SmtpEncryption::Implicit {
                SmtpTlsMode::Implicit
            } else {
                SmtpTlsMode::StartTls
            },
            username: "fixture-user".into(),
            password: "fixture-password".into(),
            from: "Chat <no-reply@example.test>".into(),
            encryption_key: [7; 32],
            fixture_transport: Some(if self.encryption == SmtpEncryption::Plaintext {
                FixtureMailTransport::PlaintextCapture
            } else {
                FixtureMailTransport::TrustedCertificate(self.certificate.clone())
            }),
        }
    }
}

impl Drop for TestSmtpServer {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}

trait FixtureIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> FixtureIo for T {}

type FixtureStream = Box<dyn FixtureIo>;

async fn capture(
    stream: TcpStream,
    encryption: SmtpEncryption,
    behavior: Arc<Mutex<SmtpBehavior>>,
    acceptor: TlsAcceptor,
    commands: Arc<Mutex<Vec<String>>>,
    messages: Arc<Mutex<Vec<CapturedMail>>>,
    acknowledge_data: Arc<Notify>,
) -> std::io::Result<()> {
    let silent = matches!(*behavior.lock().unwrap(), SmtpBehavior::Silent);
    if silent {
        std::future::pending::<()>().await;
    }

    let mut secured = encryption == SmtpEncryption::Implicit;
    let stream: FixtureStream = if secured {
        Box::new(acceptor.accept(stream).await?)
    } else {
        Box::new(stream)
    };
    let mut reader = BufReader::new(stream);
    reader
        .get_mut()
        .write_all(b"220 owned-fixture ESMTP\r\n")
        .await?;
    let mut recipient = String::new();

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(());
        }

        commands.lock().unwrap().push(line.trim_end().to_owned());
        if line.starts_with("EHLO") {
            let reply = if encryption == SmtpEncryption::StartTls && !secured {
                b"250-owned-fixture\r\n250-STARTTLS\r\n250-8BITMIME\r\n250 AUTH PLAIN\r\n"
                    .as_slice()
            } else {
                b"250-owned-fixture\r\n250-8BITMIME\r\n250 AUTH PLAIN\r\n".as_slice()
            };
            reader.get_mut().write_all(reply).await?;
        } else if line.starts_with("STARTTLS") && encryption == SmtpEncryption::StartTls && !secured
        {
            reader.get_mut().write_all(b"220 ready for TLS\r\n").await?;
            // lettre waits for 220 before TLS, so no handshake bytes precede this upgrade.
            let upgraded: FixtureStream = Box::new(acceptor.accept(reader.into_inner()).await?);
            reader = BufReader::new(upgraded);
            secured = true;
        } else if line.starts_with("AUTH") {
            reader
                .get_mut()
                .write_all(b"235 authentication accepted\r\n")
                .await?;
        } else if line.starts_with("MAIL") {
            let current_behavior = *behavior.lock().unwrap();
            let reply = match current_behavior {
                SmtpBehavior::TemporaryFailure => {
                    b"451 fixture temporarily unavailable\r\n".as_slice()
                }
                SmtpBehavior::PermanentFailure => b"550 fixture rejects sender\r\n".as_slice(),
                _ => b"250 accepted\r\n".as_slice(),
            };
            reader.get_mut().write_all(reply).await?;
        } else if line.starts_with("RCPT TO:") {
            recipient = line
                .trim_end()
                .trim_start_matches("RCPT TO:")
                .trim_matches(['<', '>'])
                .to_owned();
            reader.get_mut().write_all(b"250 accepted\r\n").await?;
        } else if line.starts_with("DATA") {
            reader.get_mut().write_all(b"354 send body\r\n").await?;
            let body = read_body(&mut reader).await?;
            let parsed = mailparse::parse_mail(body.as_bytes()).map_err(std::io::Error::other)?;
            let captured = CapturedMail {
                recipient: recipient.clone(),
                subject: parsed
                    .headers
                    .get_first_value("Subject")
                    .unwrap_or_default(),
                text: parsed.get_body().map_err(std::io::Error::other)?,
            };

            messages.lock().unwrap().push(captured);
            let pause = matches!(*behavior.lock().unwrap(), SmtpBehavior::PauseAfterData);
            if pause {
                acknowledge_data.notified().await;
            }
            reader.get_mut().write_all(b"250 accepted\r\n").await?;
        } else if line.starts_with("QUIT") {
            reader.get_mut().write_all(b"221 bye\r\n").await?;
            return Ok(());
        } else {
            reader.get_mut().write_all(b"250 accepted\r\n").await?;
        }
    }
}

async fn read_body(reader: &mut BufReader<FixtureStream>) -> std::io::Result<String> {
    let mut body = String::new();

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        if line == ".\r\n" {
            return Ok(body);
        }

        body.push_str(if line.starts_with("..") {
            &line[1..]
        } else {
            &line
        });
    }
}
